//! The resolution retention adapter's account is private: outside test
//! support it is constructed only over the process-local account, so a
//! consumer cannot wrap an arbitrary account in it and hand the workspace's
//! resident resolution state a byte quota beside the aggregate ceiling.

fn main() {
    let account = verter_session_query::retention::SemanticRetentionAccount::process_local();
    let _injected = verter_session_query::retention::ResolutionRetention(account);
}
