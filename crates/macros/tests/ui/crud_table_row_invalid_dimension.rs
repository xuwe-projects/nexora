include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct InvalidDimension {
    #[nexora(row_id, column(width = "wide"))]
    name: String,
}

fn main() {}
