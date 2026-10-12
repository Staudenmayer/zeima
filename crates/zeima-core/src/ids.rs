/// New time-sortable id (UUIDv7). Generated client-side so records can be
/// created offline on any device without collisions.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
