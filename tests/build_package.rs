use beetle::current_build_package;

#[test]
fn default_build_keeps_full_package_contract() {
    let snapshot = current_build_package();

    assert!(snapshot.default_full_package);
    assert!(snapshot.capabilities.voice);
    assert!(snapshot.capabilities.vision);
    assert!(snapshot.capabilities.sensor);
    assert!(!snapshot.profile.is_empty());
}
