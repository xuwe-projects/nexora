include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct DuplicateRowId {
    #[nexora(row_id, column)]
    id: u64,
    #[nexora(row_id, column)]
    other_id: u64,
}

fn main() {}
