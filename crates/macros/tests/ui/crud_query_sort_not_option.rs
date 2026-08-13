#[derive(Clone, nexora_macros::CrudQuery)]
struct Query {
    #[nexora(pagination)]
    page: PageQuery,
    #[nexora(sort)]
    sort: Sort,
}

#[derive(Clone)]
struct PageQuery;
#[derive(Clone)]
struct Sort;

fn main() {}
