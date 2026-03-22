#![cfg(not(target_os = "windows"))]

mod e2e {
    mod cli_diagnose;
    mod cli_hooks;
    mod cli_init;
    mod cli_install;
    mod cli_list;
    mod cli_profile;
    mod cli_remove;
    mod cli_search;
    mod cli_sync;
    mod cli_tap;
    mod cli_update;
    mod cli_use;
    mod helpers;
    mod lifecycle;
}
