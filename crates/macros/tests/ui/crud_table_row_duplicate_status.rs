include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct DuplicateStatus {
    #[nexora(row_id)]
    id: u64,
    #[nexora(column(
        status,
        status,
        render = Self::render_status,
        text = Self::status_text
    ))]
    status: String,
}

fn main() {}
