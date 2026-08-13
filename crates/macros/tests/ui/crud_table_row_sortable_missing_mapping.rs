include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct MissingSortMapping {
    #[nexora(row_id, skip)]
    id: u64,
    #[nexora(column(sortable))]
    name: String,
}

fn main() {}
