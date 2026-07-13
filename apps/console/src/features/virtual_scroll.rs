//! Console 虚拟滚动数据表功能模块。
//!
//! 该模块使用 `gpui-component` 的 `DataTable` 展示大规模股票数据，作为行列虚拟滚动、
//! 固定列、列调整、列排序和分组表头的独立导航示例。

use std::{cmp::Ordering, ops::Range};

use gpui::{
    AnyElement, App, Context, Div, Entity, IntoElement, Stateful, TextAlign, Window, div,
    prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, Size, StyleSized as _, StyledExt as _,
    table::{Column, ColumnFixed, ColumnGroup, ColumnSort, DataTable, TableDelegate, TableState},
};
use ui::Card;

const DEFAULT_ROW_COUNT: usize = 5000;

/// 虚拟滚动股票表使用的静态股票种子。
///
/// 该类型只保存股票基本身份信息，大规模表格中的价格、成交量和排名等字段会根据行号稳定生成。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StockSeed {
    symbol: &'static str,
    name: &'static str,
    market: &'static str,
}

impl StockSeed {
    const fn new(symbol: &'static str, name: &'static str, market: &'static str) -> Self {
        Self {
            symbol,
            name,
            market,
        }
    }

    /// 返回股票代码。
    ///
    /// 该值会在表格的 `Symbol` 列中和市场代码组合展示，例如 `AAPL.US`。
    pub fn symbol(self) -> &'static str {
        self.symbol
    }

    /// 返回股票名称。
    ///
    /// 该值用于表格的 `Name` 列，帮助示例接近真实证券列表的展示密度。
    pub fn name(self) -> &'static str {
        self.name
    }

    /// 返回股票所属市场。
    ///
    /// 该值用于表格的 `Market` 列，并会根据市场语义使用主题中的强调色展示。
    pub fn market(self) -> &'static str {
        self.market
    }

    fn symbol_code(self) -> String {
        format!("{}.{}", self.symbol, self.market)
    }
}

/// 返回虚拟滚动股票表用于生成行数据的股票种子。
///
/// 这些种子来自常见美股和港股示例，真实应用可以替换为后端返回的证券基础信息。
pub fn virtual_scroll_stock_seeds() -> &'static [StockSeed] {
    static SEEDS: [StockSeed; 32] = [
        StockSeed::new("AAPL", "Apple Inc.", "US"),
        StockSeed::new("MSFT", "Microsoft Corp.", "US"),
        StockSeed::new("GOOGL", "Alphabet Inc. Class A", "US"),
        StockSeed::new("AMZN", "Amazon.com Inc.", "US"),
        StockSeed::new("META", "Meta Platforms Inc.", "US"),
        StockSeed::new("TSLA", "Tesla Inc.", "US"),
        StockSeed::new("NVDA", "NVIDIA Corp.", "US"),
        StockSeed::new("JPM", "JPMorgan Chase & Co.", "US"),
        StockSeed::new("V", "Visa Inc.", "US"),
        StockSeed::new("UNH", "UnitedHealth Group Inc.", "US"),
        StockSeed::new("MA", "Mastercard Inc.", "US"),
        StockSeed::new("HD", "Home Depot Inc.", "US"),
        StockSeed::new("PG", "Procter & Gamble Co.", "US"),
        StockSeed::new("LLY", "Eli Lilly and Co.", "US"),
        StockSeed::new("BAC", "Bank of America Corp.", "US"),
        StockSeed::new("XOM", "Exxon Mobil Corp.", "US"),
        StockSeed::new("KO", "Coca-Cola Co.", "US"),
        StockSeed::new("MRK", "Merck & Co. Inc.", "US"),
        StockSeed::new("PEP", "PepsiCo Inc.", "US"),
        StockSeed::new("ABBV", "AbbVie Inc.", "US"),
        StockSeed::new("AVGO", "Broadcom Inc.", "US"),
        StockSeed::new("COST", "Costco Wholesale Corp.", "US"),
        StockSeed::new("WMT", "Walmart Inc.", "US"),
        StockSeed::new("MCD", "McDonald's Corp.", "US"),
        StockSeed::new("ADBE", "Adobe Inc.", "US"),
        StockSeed::new("0883", "CNOOC Ltd.", "HK"),
        StockSeed::new("0700", "Tencent Holdings Ltd.", "HK"),
        StockSeed::new("9988", "Alibaba Group Holding Ltd.", "HK"),
        StockSeed::new("3690", "Meituan Class B", "HK"),
        StockSeed::new("1810", "Xiaomi Corp.", "HK"),
        StockSeed::new("1299", "AIA Group Ltd.", "HK"),
        StockSeed::new("0005", "HSBC Holdings plc", "HK"),
    ];

    &SEEDS
}

/// 虚拟滚动数据表功能视图。
///
/// 该 feature 持有独立的 `TableState`，用于展示大规模股票行和多列横向滚动，避免把演示状态放进根视图。
#[derive(Default)]
pub struct VirtualScrollFeature {
    table: Option<Entity<TableState<StockTableDelegate>>>,
}

impl VirtualScrollFeature {
    /// 渲染虚拟滚动股票数据表。
    ///
    /// 页面展示标题、当前表格规模摘要和 `DataTable` 主体，表格本身支持行列虚拟滚动、固定列、
    /// 列调整、列拖拽排序和点击表头排序。
    pub fn render<T>(&mut self, window: &mut Window, cx: &mut Context<T>) -> AnyElement
    where
        T: 'static,
    {
        let table = self.table(window, cx);
        let table_state = table.read(cx);
        let delegate = table_state.delegate();
        let theme = cx.theme();

        Card::new()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .text_sm()
            .p_4()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_4()
                    .flex_wrap()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .font_bold()
                                    .text_color(theme.foreground)
                                    .child("DataTable"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("A complex data table with selection, sorting, column moving, and virtual scrolling."),
                            ),
                    )
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .flex_wrap()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(format!("Total Rows: {}", delegate.rows_count(cx)))
                    .child(format!(
                        "Visible Rows: {}..{}",
                        delegate.visible_rows.start, delegate.visible_rows.end
                    ))
                    .child(format!(
                        "Visible Cols: {}..{}",
                        delegate.visible_cols.start, delegate.visible_cols.end
                    ))
                    .child(format!("Stocks: {}", delegate.rows_count(cx)))
                    .child(format!("Columns: {}", delegate.columns_count(cx)))
                    .child("Fixed columns, group headers, column resize, column order, sortable"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .w_full()
                    .child(
                        DataTable::new(&table)
                            .with_size(Size::Medium)
                            .stripe(true)
                            .scrollbar_visible(true, true),
                    ),
            )
            .into_any_element()
    }

    fn table<T>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Entity<TableState<StockTableDelegate>>
    where
        T: 'static,
    {
        self.table
            .get_or_insert_with(|| {
                cx.new(|cx| {
                    TableState::new(StockTableDelegate::new(DEFAULT_ROW_COUNT), window, cx)
                        .loop_selection(true)
                        .col_resizable(true)
                        .col_movable(true)
                        .sortable(true)
                        .col_selectable(true)
                        .row_selectable(true)
                        .cell_selectable(false)
                        .row_header(false)
                })
            })
            .clone()
    }
}

#[derive(Debug, Clone)]
struct Stock {
    id: usize,
    seed: StockSeed,
    price: f64,
    change: f64,
    change_percent: f64,
    volume: f64,
    turnover: f64,
    market_cap: f64,
    ttm: f64,
    five_mins_ranking: f64,
    th60_days_ranking: f64,
    year_change_percent: f64,
    bid: f64,
    bid_volume: f64,
    ask: f64,
    ask_volume: f64,
    open: f64,
    prev_close: f64,
    high: f64,
    low: f64,
    turnover_rate: f64,
    rise_rate: f64,
    amplitude: f64,
    pe_status: f64,
    pb_status: f64,
    volume_ratio: f64,
    bid_ask_ratio: f64,
    latest_pre_close: f64,
    latest_post_close: f64,
    pre_market_cap: f64,
    pre_market_percent: f64,
    pre_market_change: f64,
    post_market_cap: f64,
    post_market_percent: f64,
    post_market_change: f64,
    float_cap: f64,
    shares: i64,
    shares_float: i64,
    day_5_ranking: f64,
    day_10_ranking: f64,
    day_30_ranking: f64,
}

impl Stock {
    fn generated(id: usize) -> Self {
        let seed = virtual_scroll_stock_seeds()[id % virtual_scroll_stock_seeds().len()];
        let price = value_for(id, 1, 12.0, 988.0);
        let change = value_for(id, 2, -90.0, 180.0);
        let bid = price * (1.0 + value_for(id, 14, -0.12, 0.24));
        let ask = price * (1.0 + value_for(id, 16, -0.08, 0.22));

        Self {
            id,
            seed,
            price,
            change,
            change_percent: value_for(id, 3, -0.1, 0.2),
            volume: value_for(id, 4, 10.0, 990.0),
            turnover: value_for(id, 5, 10.0, 990.0),
            market_cap: value_for(id, 6, 40.0, 960.0),
            ttm: value_for(id, 7, 20.0, 980.0),
            five_mins_ranking: value_for(id, 8, 0.0, 1000.0),
            th60_days_ranking: value_for(id, 9, 0.0, 1000.0),
            year_change_percent: value_for(id, 10, -1.0, 2.0),
            bid,
            bid_volume: value_for(id, 15, 100.0, 900.0),
            ask,
            ask_volume: value_for(id, 17, 100.0, 900.0),
            open: value_for(id, 18, 12.0, 988.0),
            prev_close: value_for(id, 19, 12.0, 988.0),
            high: price * (1.0 + value_for(id, 20, 0.0, 0.3)),
            low: price * (1.0 - value_for(id, 21, 0.0, 0.25)),
            turnover_rate: value_for(id, 22, 0.0, 1.0),
            rise_rate: value_for(id, 23, 0.0, 1.0),
            amplitude: value_for(id, 24, 0.0, 1000.0),
            pe_status: value_for(id, 25, 0.0, 1000.0),
            pb_status: value_for(id, 26, 0.0, 1000.0),
            volume_ratio: value_for(id, 27, 0.0, 1.0),
            bid_ask_ratio: bid / ask.max(0.001),
            latest_pre_close: value_for(id, 28, 0.0, 1000.0),
            latest_post_close: value_for(id, 29, 0.0, 1000.0),
            pre_market_cap: value_for(id, 30, 0.0, 1000.0),
            pre_market_percent: value_for(id, 31, -1.0, 2.0),
            pre_market_change: value_for(id, 32, -100.0, 200.0),
            post_market_cap: value_for(id, 33, 0.0, 1000.0),
            post_market_percent: value_for(id, 34, -1.0, 2.0),
            post_market_change: value_for(id, 35, -100.0, 200.0),
            float_cap: value_for(id, 36, 0.0, 1000.0),
            shares: whole_for(id, 37, 100_000, 9_900_000),
            shares_float: whole_for(id, 38, 100_000, 9_900_000),
            day_5_ranking: value_for(id, 39, 0.0, 1000.0),
            day_10_ranking: value_for(id, 40, 0.0, 1000.0),
            day_30_ranking: value_for(id, 41, 0.0, 1000.0),
        }
    }
}

#[derive(Debug, Clone)]
struct StockTableDelegate {
    stocks: Vec<Stock>,
    columns: Vec<Column>,
    size: Size,
    visible_rows: Range<usize>,
    visible_cols: Range<usize>,
}

impl StockTableDelegate {
    fn new(size: usize) -> Self {
        Self {
            stocks: (0..size).map(Stock::generated).collect(),
            columns: stock_columns(),
            size: Size::Medium,
            visible_rows: Range::default(),
            visible_cols: Range::default(),
        }
    }

    fn render_number(&self, col: &Column, value: f64, cx: &App) -> AnyElement {
        let bucket = value_bucket(value);

        div()
            .h_full()
            .table_cell_size(self.size)
            .child(format!("{value:.3}"))
            .when(col.align == TextAlign::Right, |this| {
                this.flex().items_center().justify_end()
            })
            .when(bucket == 0, |this| {
                this.text_color(cx.theme().red)
                    .bg(cx.theme().red_light.alpha(0.05))
            })
            .when(bucket == 1, |this| {
                this.text_color(cx.theme().green)
                    .bg(cx.theme().green_light.alpha(0.05))
            })
            .into_any_element()
    }

    fn render_percent(&self, col: &Column, value: f64, cx: &App) -> AnyElement {
        let bucket = value_bucket(value);

        div()
            .h_full()
            .table_cell_size(self.size)
            .child(format!("{:.2}%", value * 100.0))
            .when(col.align == TextAlign::Right, |this| {
                this.flex().items_center().justify_end()
            })
            .when(bucket == 0, |this| {
                this.text_color(cx.theme().red)
                    .bg(cx.theme().red_light.alpha(0.05))
            })
            .when(bucket == 1, |this| {
                this.text_color(cx.theme().green)
                    .bg(cx.theme().green_light.alpha(0.05))
            })
            .into_any_element()
    }

    fn render_plain(&self, col: &Column, value: impl IntoElement, cx: &App) -> AnyElement {
        div()
            .h_full()
            .table_cell_size(self.size)
            .text_color(cx.theme().foreground)
            .child(value)
            .when(col.align == TextAlign::Center, |this| {
                this.flex().items_center().justify_center()
            })
            .when(col.align == TextAlign::Right, |this| {
                this.flex().items_center().justify_end()
            })
            .into_any_element()
    }

    fn text_for(&self, row_ix: usize, col_ix: usize) -> String {
        let Some(stock) = self.stocks.get(row_ix) else {
            return String::new();
        };
        let Some(col) = self.columns.get(col_ix) else {
            return String::new();
        };

        stock_text(stock, col.key.as_ref())
    }
}

impl TableDelegate for StockTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.stocks.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn group_headers(&self, _: &App) -> Option<Vec<Vec<ColumnGroup>>> {
        Some(vec![
            vec![
                ColumnGroup::new("Stock Info", 4),
                ColumnGroup::new("Price & Change", 3),
            ],
            vec![
                ColumnGroup::new("Identity", 4),
                ColumnGroup::new("Stock Info", 7),
                ColumnGroup::new("Ranking & Stats", 14),
                ColumnGroup::new("Market Data", self.columns.len() - 25),
            ],
        ])
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let col = self.column(col_ix, cx);

        div()
            .size_full()
            .font_medium()
            .text_color(cx.theme().table_head_foreground)
            .child(col.name.clone())
            .when(col.align == TextAlign::Center, |this| {
                this.flex().items_center().justify_center()
            })
            .when(col.align == TextAlign::Right, |this| {
                this.flex().items_center().justify_end()
            })
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let stock_id = self.stocks.get(row_ix).map_or(0, |stock| stock.id);
        div().id(("stock-row", stock_id))
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(stock) = self.stocks.get(row_ix) else {
            return "--".into_any_element();
        };
        let Some(col) = self.columns.get(col_ix) else {
            return "--".into_any_element();
        };

        match col.key.as_ref() {
            "id" => self.render_plain(col, stock.id.to_string(), cx),
            "market" => div()
                .h_full()
                .table_cell_size(self.size)
                .text_color(if stock.seed.market() == "US" {
                    cx.theme().blue
                } else {
                    cx.theme().magenta
                })
                .child(stock.seed.market())
                .into_any_element(),
            "name" => self.render_plain(col, stock.seed.name(), cx),
            "symbol" => self.render_plain(col, stock.seed.symbol_code(), cx),
            "price" => self.render_number(col, stock.price, cx),
            "change" => self.render_number(col, stock.change, cx),
            "change_percent" => self.render_percent(col, stock.change_percent, cx),
            "volume" => self.render_number(col, stock.volume, cx),
            "turnover" => self.render_number(col, stock.turnover, cx),
            "market_cap" => self.render_number(col, stock.market_cap, cx),
            "ttm" => self.render_number(col, stock.ttm, cx),
            "five_mins_ranking" => self.render_number(col, stock.five_mins_ranking, cx),
            "th60_days_ranking" => {
                self.render_plain(col, stock.th60_days_ranking.floor().to_string(), cx)
            }
            "year_change_percent" => self.render_percent(col, stock.year_change_percent, cx),
            "bid" => self.render_number(col, stock.bid, cx),
            "bid_volume" => self.render_number(col, stock.bid_volume, cx),
            "ask" => self.render_number(col, stock.ask, cx),
            "ask_volume" => self.render_number(col, stock.ask_volume, cx),
            "open" => self.render_number(col, stock.open, cx),
            "prev_close" => self.render_number(col, stock.prev_close, cx),
            "high" => self.render_number(col, stock.high, cx),
            "low" => self.render_number(col, stock.low, cx),
            "turnover_rate" => self.render_percent(col, stock.turnover_rate, cx),
            "rise_rate" => self.render_percent(col, stock.rise_rate, cx),
            "amplitude" => self.render_number(col, stock.amplitude, cx),
            "pe_status" => self.render_plain(col, stock.pe_status.floor().to_string(), cx),
            "pb_status" => self.render_plain(col, stock.pb_status.floor().to_string(), cx),
            "volume_ratio" => self.render_number(col, stock.volume_ratio, cx),
            "bid_ask_ratio" => self.render_number(col, stock.bid_ask_ratio, cx),
            "latest_pre_close" => {
                self.render_plain(col, stock.latest_pre_close.floor().to_string(), cx)
            }
            "latest_post_close" => {
                self.render_plain(col, stock.latest_post_close.floor().to_string(), cx)
            }
            "pre_market_cap" => {
                self.render_plain(col, stock.pre_market_cap.floor().to_string(), cx)
            }
            "pre_market_percent" => self.render_percent(col, stock.pre_market_percent, cx),
            "pre_market_change" => {
                self.render_plain(col, stock.pre_market_change.floor().to_string(), cx)
            }
            "post_market_cap" => {
                self.render_plain(col, stock.post_market_cap.floor().to_string(), cx)
            }
            "post_market_percent" => self.render_percent(col, stock.post_market_percent, cx),
            "post_market_change" => {
                self.render_plain(col, stock.post_market_change.floor().to_string(), cx)
            }
            "float_cap" => self.render_plain(col, stock.float_cap.floor().to_string(), cx),
            "shares" => self.render_plain(col, stock.shares.to_string(), cx),
            "shares_float" => self.render_plain(col, stock.shares_float.to_string(), cx),
            "day_5_ranking" => self.render_plain(col, stock.day_5_ranking.floor().to_string(), cx),
            "day_10_ranking" => {
                self.render_plain(col, stock.day_10_ranking.floor().to_string(), cx)
            }
            "day_30_ranking" => {
                self.render_plain(col, stock.day_30_ranking.floor().to_string(), cx)
            }
            _ => "--".into_any_element(),
        }
    }

    fn move_column(
        &mut self,
        col_ix: usize,
        to_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        if col_ix >= self.columns.len() || to_ix >= self.columns.len() {
            return;
        }

        let column = self.columns.remove(col_ix);
        self.columns.insert(to_ix, column);
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        let Some(col) = self.columns.get(col_ix) else {
            return;
        };

        match col.key.as_ref() {
            "id" => self
                .stocks
                .sort_by(|left, right| sort_order(sort, left.id.cmp(&right.id))),
            "market" => self.stocks.sort_by(|left, right| {
                sort_order(sort, left.seed.market().cmp(right.seed.market()))
            }),
            "name" => self
                .stocks
                .sort_by(|left, right| sort_order(sort, left.seed.name().cmp(right.seed.name()))),
            "symbol" => self.stocks.sort_by(|left, right| {
                sort_order(sort, left.seed.symbol().cmp(right.seed.symbol()))
            }),
            "price" => self
                .stocks
                .sort_by(|left, right| sort_f64(sort, left.price, right.price)),
            "change" => self
                .stocks
                .sort_by(|left, right| sort_f64(sort, left.change, right.change)),
            "change_percent" => self
                .stocks
                .sort_by(|left, right| sort_f64(sort, left.change_percent, right.change_percent)),
            "volume" => self
                .stocks
                .sort_by(|left, right| sort_f64(sort, left.volume, right.volume)),
            "turnover" => self
                .stocks
                .sort_by(|left, right| sort_f64(sort, left.turnover, right.turnover)),
            "market_cap" => self
                .stocks
                .sort_by(|left, right| sort_f64(sort, left.market_cap, right.market_cap)),
            _ => {}
        }
    }

    fn visible_rows_changed(
        &mut self,
        visible_range: Range<usize>,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        self.visible_rows = visible_range;
    }

    fn visible_columns_changed(
        &mut self,
        visible_range: Range<usize>,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        self.visible_cols = visible_range;
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        self.text_for(row_ix, col_ix)
    }
}

fn stock_columns() -> Vec<Column> {
    vec![
        Column::new("id", "ID")
            .width(60.)
            .fixed(ColumnFixed::Left)
            .resizable(true)
            .min_width(40.)
            .max_width(100.)
            .text_center(),
        Column::new("market", "Market")
            .width(70.)
            .fixed(ColumnFixed::Left)
            .resizable(true)
            .min_width(56.),
        Column::new("name", "Name")
            .width(190.)
            .fixed(ColumnFixed::Left)
            .resizable(true)
            .max_width(320.),
        Column::new("symbol", "Symbol")
            .width(110.)
            .fixed(ColumnFixed::Left)
            .sortable(),
        Column::new("price", "Price").sortable().text_right().p_0(),
        Column::new("change", "Chg").sortable().text_right().p_0(),
        Column::new("change_percent", "Chg%")
            .sortable()
            .text_right()
            .p_0(),
        Column::new("volume", "Volume").sortable().p_0(),
        Column::new("turnover", "Turnover").sortable().p_0(),
        Column::new("market_cap", "Market Cap").sortable().p_0(),
        Column::new("ttm", "TTM").p_0(),
        Column::new("five_mins_ranking", "5m Ranking")
            .text_right()
            .p_0(),
        Column::new("th60_days_ranking", "60d Ranking"),
        Column::new("year_change_percent", "Year Chg%").text_right(),
        Column::new("bid", "Bid").text_right().p_0(),
        Column::new("bid_volume", "Bid Vol").text_right().p_0(),
        Column::new("ask", "Ask").text_right().p_0(),
        Column::new("ask_volume", "Ask Vol").text_right().p_0(),
        Column::new("open", "Open").text_right().p_0(),
        Column::new("prev_close", "Prev Close").text_right().p_0(),
        Column::new("high", "High").text_right().p_0(),
        Column::new("low", "Low").text_right().p_0(),
        Column::new("turnover_rate", "Turnover Rate").text_right(),
        Column::new("rise_rate", "Rise Rate").text_right(),
        Column::new("amplitude", "Amplitude").text_right(),
        Column::new("pe_status", "P/E").text_right(),
        Column::new("pb_status", "P/B").text_right(),
        Column::new("volume_ratio", "Volume Ratio")
            .text_right()
            .p_0(),
        Column::new("bid_ask_ratio", "Bid Ask Ratio")
            .text_right()
            .p_0(),
        Column::new("latest_pre_close", "Latest Pre Close"),
        Column::new("latest_post_close", "Latest Post Close"),
        Column::new("pre_market_cap", "Pre Mkt Cap"),
        Column::new("pre_market_percent", "Pre Mkt%").text_right(),
        Column::new("pre_market_change", "Pre Mkt Chg"),
        Column::new("post_market_cap", "Post Mkt Cap"),
        Column::new("post_market_percent", "Post Mkt%").text_right(),
        Column::new("post_market_change", "Post Mkt Chg"),
        Column::new("float_cap", "Float Cap"),
        Column::new("shares", "Shares"),
        Column::new("shares_float", "Float Shares"),
        Column::new("day_5_ranking", "5d Ranking"),
        Column::new("day_10_ranking", "10d Ranking"),
        Column::new("day_30_ranking", "30d Ranking"),
    ]
}

fn value_for(row_ix: usize, salt: u64, start: f64, span: f64) -> f64 {
    let mixed = (row_ix as u64 + 1)
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(salt.wrapping_mul(1_442_695_040_888_963_407));
    let fraction = (mixed % 1_000_000) as f64 / 1_000_000.0;

    start + span * fraction
}

fn whole_for(row_ix: usize, salt: u64, start: i64, span: i64) -> i64 {
    start + value_for(row_ix, salt, 0.0, span as f64).floor() as i64
}

fn value_bucket(value: f64) -> i32 {
    ((value.abs().fract() * 1000.0).floor() as i32) % 3
}

fn sort_order(sort: ColumnSort, ordering: Ordering) -> Ordering {
    match sort {
        ColumnSort::Descending => ordering.reverse(),
        _ => ordering,
    }
}

fn sort_f64(sort: ColumnSort, left: f64, right: f64) -> Ordering {
    sort_order(sort, left.partial_cmp(&right).unwrap_or(Ordering::Equal))
}

fn stock_text(stock: &Stock, key: &str) -> String {
    match key {
        "id" => stock.id.to_string(),
        "market" => stock.seed.market().to_string(),
        "name" => stock.seed.name().to_string(),
        "symbol" => stock.seed.symbol_code(),
        "price" => format!("{:.3}", stock.price),
        "change" => format!("{:.3}", stock.change),
        "change_percent" => format!("{:.2}%", stock.change_percent * 100.0),
        "volume" => format!("{:.3}", stock.volume),
        "turnover" => format!("{:.3}", stock.turnover),
        "market_cap" => format!("{:.3}", stock.market_cap),
        "ttm" => format!("{:.3}", stock.ttm),
        "five_mins_ranking" => format!("{:.3}", stock.five_mins_ranking),
        "th60_days_ranking" => stock.th60_days_ranking.floor().to_string(),
        "year_change_percent" => format!("{:.2}%", stock.year_change_percent * 100.0),
        "bid" => format!("{:.3}", stock.bid),
        "bid_volume" => format!("{:.3}", stock.bid_volume),
        "ask" => format!("{:.3}", stock.ask),
        "ask_volume" => format!("{:.3}", stock.ask_volume),
        "open" => format!("{:.3}", stock.open),
        "prev_close" => format!("{:.3}", stock.prev_close),
        "high" => format!("{:.3}", stock.high),
        "low" => format!("{:.3}", stock.low),
        "turnover_rate" => format!("{:.2}%", stock.turnover_rate * 100.0),
        "rise_rate" => format!("{:.2}%", stock.rise_rate * 100.0),
        "amplitude" => format!("{:.3}", stock.amplitude),
        "pe_status" => stock.pe_status.floor().to_string(),
        "pb_status" => stock.pb_status.floor().to_string(),
        "volume_ratio" => format!("{:.3}", stock.volume_ratio),
        "bid_ask_ratio" => format!("{:.3}", stock.bid_ask_ratio),
        "latest_pre_close" => stock.latest_pre_close.floor().to_string(),
        "latest_post_close" => stock.latest_post_close.floor().to_string(),
        "pre_market_cap" => stock.pre_market_cap.floor().to_string(),
        "pre_market_percent" => format!("{:.2}%", stock.pre_market_percent * 100.0),
        "pre_market_change" => stock.pre_market_change.floor().to_string(),
        "post_market_cap" => stock.post_market_cap.floor().to_string(),
        "post_market_percent" => format!("{:.2}%", stock.post_market_percent * 100.0),
        "post_market_change" => stock.post_market_change.floor().to_string(),
        "float_cap" => stock.float_cap.floor().to_string(),
        "shares" => stock.shares.to_string(),
        "shares_float" => stock.shares_float.to_string(),
        "day_5_ranking" => stock.day_5_ranking.floor().to_string(),
        "day_10_ranking" => stock.day_10_ranking.floor().to_string(),
        "day_30_ranking" => stock.day_30_ranking.floor().to_string(),
        _ => String::new(),
    }
}
