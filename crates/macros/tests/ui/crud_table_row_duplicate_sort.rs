include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct DuplicateSort {
    #[nexora(column(sortable, ascending))]
    name: String,
}

fn main() {}
