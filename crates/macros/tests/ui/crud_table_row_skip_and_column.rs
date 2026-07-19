include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct SkipAndColumn {
    #[nexora(skip, column)]
    name: String,
}

fn main() {}
