include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct StatusMissingRender {
    #[nexora(row_id)]
    id: u64,
    #[nexora(column(status, text = Self::status_text))]
    status: String,
}

fn main() {}
