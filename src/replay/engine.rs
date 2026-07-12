pub fn replay_validate(db: &str, task_id: &str) -> bool {
    if !std::path::Path::new(db).exists() {
        return true;
    }
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().replay_validate(task_id);
    crate::providers::get_storage().set_override_path(None);
    res
}
