#![cfg(not(target_os = "windows"))]

mod e2e {
    mod cli_install;
    mod cli_list;
    mod cli_remove;
    mod cli_search;
    mod cli_update;
    mod helpers;
    mod lifecycle;
}
