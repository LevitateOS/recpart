use clap::Parser;
use recpart::cli::{Cli, Commands, ModeArg};

#[test]
fn plan_mode_defaults_to_ab() {
    let cli = Cli::parse_from(["recpart", "plan", "--disk", "/dev/vda"]);
    match cli.command {
        Some(Commands::Plan(args)) => assert!(matches!(args.mode, ModeArg::Ab)),
        _ => panic!("expected plan command"),
    }
}

#[test]
fn apply_accepts_layout_request_flags() {
    let cli = Cli::parse_from([
        "recpart",
        "apply",
        "--disk",
        "/dev/vda",
        "--mode",
        "ab",
        "--efi-size-mb",
        "512",
        "--root-a-size-mb",
        "10240",
        "--root-b-size-mb",
        "10240",
        "--state-size-mb",
        "8192",
        "--dry-run",
    ]);

    match cli.command {
        Some(Commands::Apply(args)) => {
            assert_eq!(args.layout.efi_size_mb, Some(512));
            assert_eq!(args.layout.root_a_size_mb, Some(10_240));
            assert_eq!(args.layout.root_b_size_mb, Some(10_240));
            assert_eq!(args.layout.state_size_mb, Some(8_192));
        }
        _ => panic!("expected apply command"),
    }
}
