#[derive(Clone, nexora_macros::CrudQuery)]
#[nexora(page_size(default = 20, options = [15, 25, 50]))]
struct Query {
    #[nexora(pagination)]
    page: PageQuery,
}

#[derive(Clone)]
struct PageQuery;

fn main() {}
