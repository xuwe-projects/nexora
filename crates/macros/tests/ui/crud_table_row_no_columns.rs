include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct NoColumns {
    id: u64,
}

fn main() {}
