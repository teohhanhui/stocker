use crate::app::App;
use chrono::{Duration, TimeZone, Utc};
use math::round;
use std::cmp::Ordering;
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Text},
    Frame,
};
use yahoo_finance::{Profile, Timestamped};

pub fn draw<B: Backend>(f: &mut Frame<B>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(f.size());

    draw_header(f, app, chunks[0]);
    draw_body(f, app, chunks[1]);
    draw_footer(f, app, chunks[2]);
}

fn draw_header<B: Backend>(f: &mut Frame<B>, App { stock, .. }: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .horizontal_margin(1)
        .constraints(vec![Constraint::Length(10), Constraint::Length(20)])
        .split(area);

    let header_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);

    let header_block = Block::default().style(header_base_style);
    f.render_widget(header_block, area);

    let symbol_texts = vec![Text::raw(stock.symbol.clone())];
    let symbol_paragraph = Paragraph::new(symbol_texts.iter())
        .block(Block::default().style(header_base_style))
        .style(header_base_style.clone().modifier(Modifier::BOLD));
    f.render_widget(symbol_paragraph, chunks[0]);

    let name_texts = vec![Text::raw(match &stock.profile {
        Some(Profile::Company(company)) => company.name.clone(),
        Some(Profile::Fund(fund)) => fund.name.clone(),
        None => "".to_owned(),
    })];
    let name_paragraph = Paragraph::new(name_texts.iter())
        .block(Block::default().style(header_base_style))
        .style(header_base_style);
    f.render_widget(name_paragraph, chunks[1]);
}

fn draw_body<B: Backend>(f: &mut Frame<B>, App { stock, .. }: &App, area: Rect) {
    let historical_prices_data = stock
        .bars
        .iter()
        .map(|bar| {
            (
                bar.timestamp_seconds() as f64,
                round::half_to_even(bar.close, 2),
            )
        })
        .collect::<Vec<_>>();
    let historical_prices_datasets = [Dataset::default()
        .marker(Marker::Braille)
        .style(Style::default().fg(Color::Cyan))
        .graph_type(GraphType::Line)
        .data(&historical_prices_data)];
    let min_timestamp = historical_prices_data
        .clone()
        .into_iter()
        .map(|(date, _)| date)
        .min_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal))
        .unwrap_or((Utc::now() - Duration::days(7)).timestamp() as f64);
    let price_limit_low = historical_prices_data
        .clone()
        .into_iter()
        .map(|(_, price)| price)
        .min_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal))
        .unwrap_or(0_f64);
    let price_limit_low = round::floor(price_limit_low, 0);
    let price_limit_high = historical_prices_data
        .clone()
        .into_iter()
        .map(|(_, price)| price)
        .max_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal))
        .unwrap_or(1_000_000_f64);
    let price_limit_high = round::ceil(price_limit_high, 0);

    let x_axis_bounds = [min_timestamp, Utc::now().timestamp() as f64];
    let x_axis_labels = [
        Utc.timestamp(min_timestamp as i64, 0)
            .format("%Y-%m-%d")
            .to_string(),
        Utc::now().format("%Y-%m-%d").to_string(),
    ];
    let y_axis_bounds = [price_limit_low, price_limit_high];
    let y_axis_labels = [price_limit_low.to_string(), price_limit_high.to_string()];

    let historical_prices_chart = Chart::default()
        .block(
            Block::default()
                .title("Historical Prices")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Gray)),
        )
        .x_axis(Axis::default().bounds(x_axis_bounds).labels(&x_axis_labels))
        .y_axis(Axis::default().bounds(y_axis_bounds).labels(&y_axis_labels))
        .datasets(&historical_prices_datasets);
    f.render_widget(historical_prices_chart, area);
}

fn draw_footer<B: Backend>(f: &mut Frame<B>, App { ui_state, .. }: &App, area: Rect) {
    let time_frame_texts = vec![
        Text::styled("Time frame: ", Style::default().fg(Color::Gray)),
        Text::raw(ui_state.time_frame.to_string()),
    ];
    let time_frame_paragraph = Paragraph::new(time_frame_texts.iter())
        .block(Block::default())
        .alignment(Alignment::Right);
    f.render_widget(time_frame_paragraph, area);
}
