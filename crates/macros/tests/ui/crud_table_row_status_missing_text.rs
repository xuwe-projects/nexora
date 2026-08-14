include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct StatusMissingText {
    #[nexora(row_id)]
    id: u64,
    #[nexora(column(status, render = Self::render_status))]
    status: String,
}

fn main() {}
