include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct MissingRowId {
    #[nexora(column)]
    name: String,
}

fn main() {}
