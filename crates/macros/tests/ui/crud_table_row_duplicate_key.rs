include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct DuplicateKey {
    #[nexora(column(key = "id"))]
    id: u64,
    #[nexora(column(key = "id"))]
    other_id: u64,
}

fn main() {}
