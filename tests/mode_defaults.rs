use recpart::types::InstallMode;

#[test]
fn default_mode_is_ab() {
    assert_eq!(InstallMode::default(), InstallMode::Ab);
}
