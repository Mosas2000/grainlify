#![cfg(test)]

use crate::{monitoring, DataKey, GrainlifyContract, GrainlifyContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String as SdkString, Symbol,
};

fn setup_contract(env: &Env) -> (GrainlifyContractClient<'_>, Address) {
    let contract_id = env.register_contract(None, GrainlifyContract);
    let client = GrainlifyContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.init_admin(&admin);
    (client, admin)
}

#[test]
fn test_monitoring_views_are_safe_on_empty_state() {
    let env = Env::default();
    let contract_id = env.register_contract(None, GrainlifyContract);
    let client = GrainlifyContractClient::new(&env, &contract_id);

    let health = client.health_check();
    assert!(!health.is_healthy);
    assert_eq!(health.last_operation, 0);
    assert_eq!(health.total_operations, 0);
    assert_eq!(health.contract_version, SdkString::from_str(&env, "0.0.0"));

    let analytics = client.get_analytics();
    assert_eq!(analytics.operation_count, 0);
    assert_eq!(analytics.unique_users, 0);
    assert_eq!(analytics.error_count, 0);
    assert_eq!(analytics.error_rate, 0);
}

#[test]
fn test_monitoring_views_report_initialized_state() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|ledger| ledger.timestamp = 7);

    let (client, _admin) = setup_contract(&env);

    let health = client.health_check();
    assert!(health.is_healthy);
    assert_eq!(health.last_operation, 7);
    assert_eq!(health.total_operations, 1);
    assert_eq!(health.contract_version, SdkString::from_str(&env, "2.0.0"));

    let analytics = client.get_analytics();
    assert_eq!(analytics.operation_count, 1);
    assert_eq!(analytics.unique_users, 1);
    assert_eq!(analytics.error_count, 0);
    assert_eq!(analytics.error_rate, 0);
}

#[test]
fn test_monitoring_unique_user_count_is_bounded() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup_contract(&env);

    env.ledger().with_mut(|ledger| ledger.timestamp = 99);
    env.as_contract(&client.address, || {
        for index in 0..(monitoring::MAX_TRACKED_USERS + 5) {
            let caller = Address::generate(&env);
            let operation = Symbol::new(&env, if index % 2 == 0 { "ping" } else { "pong" });
            monitoring::track_operation(&env, operation, caller, true);
        }
    });

    let health = client.health_check();
    assert_eq!(health.last_operation, 99);
    assert_eq!(health.total_operations, monitoring::MAX_TRACKED_USERS as u64 + 6);

    let analytics = client.get_analytics();
    assert_eq!(analytics.operation_count, monitoring::MAX_TRACKED_USERS as u64 + 6);
    assert_eq!(analytics.unique_users, monitoring::MAX_TRACKED_USERS as u64);
    assert_eq!(analytics.error_count, 0);
    assert_eq!(analytics.error_rate, 0);
}

#[test]
fn test_check_invariants_healthy_after_init() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup_contract(&env);

    let report = client.check_invariants();
    assert!(report.healthy);
    assert!(report.config_sane);
    assert!(report.metrics_sane);
    assert!(report.admin_set);
    assert!(report.version_set);
    assert_eq!(report.violation_count, 0);
    assert!(client.verify_invariants());
}

#[test]
fn test_check_invariants_detects_metric_drift() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup_contract(&env);

    env.as_contract(&client.address, || {
        let op_key = Symbol::new(&env, "op_count");
        let err_key = Symbol::new(&env, "err_count");
        env.storage().persistent().set(&op_key, &2_u64);
        env.storage().persistent().set(&err_key, &5_u64);
    });

    let report = client.check_invariants();
    assert!(report.config_sane);
    assert!(!report.metrics_sane);
    assert!(!report.healthy);
    assert!(report.violation_count > 0);
    assert!(!client.verify_invariants());
}

#[test]
fn test_check_invariants_detects_config_drift() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup_contract(&env);

    env.as_contract(&client.address, || {
        env.storage().instance().remove(&DataKey::Version);
    });

    let report = client.check_invariants();
    assert!(!report.config_sane);
    assert!(!report.healthy);
    assert!(report.violation_count > 0);
    assert!(!client.verify_invariants());
}
