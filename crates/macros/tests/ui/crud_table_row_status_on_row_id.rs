include!("crud_table_support.rs");

#[derive(Clone, nexora_macros::CrudTableRow)]
struct StatusOnRowId {
    #[nexora(
        row_id,
        column(
            status,
            render = Self::render_status,
            text = Self::status_text
        )
    )]
    id: u64,
}

fn main() {}
