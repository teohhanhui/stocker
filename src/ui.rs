use crate::app::{App, TimeFrame};
use chrono::{TimeZone, Utc};
use math::round;
use std::cmp::{self, Ordering};
use strum::IntoEnumIterator;
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    widgets::{Axis, Block, Borders, Chart, Clear, Dataset, GraphType, List, Paragraph, Text},
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
    let header_area = chunks[0];
    let body_area = chunks[1];
    let footer_area = chunks[2];

    draw_header(f, app, header_area);
    draw_body(f, app, body_area);
    draw_footer(f, app, footer_area);
}

fn draw_header<B: Backend>(f: &mut Frame<B>, App { stock, .. }: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .horizontal_margin(1)
        .constraints(vec![Constraint::Length(10), Constraint::Length(20)])
        .split(area);
    let symbol_area = chunks[0];
    let name_area = chunks[1];

    let header_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);

    let header_block = Block::default().style(header_base_style);
    f.render_widget(header_block, area);

    let symbol_texts = vec![Text::raw(stock.symbol.clone())];
    let symbol_paragraph = Paragraph::new(symbol_texts.iter())
        .block(Block::default().style(header_base_style))
        .style(header_base_style.clone().modifier(Modifier::BOLD));
    f.render_widget(symbol_paragraph, symbol_area);

    let name_texts = vec![Text::raw(match &stock.profile {
        Some(Profile::Company(company)) => company.name.clone(),
        Some(Profile::Fund(fund)) => fund.name.clone(),
        None => "".to_owned(),
    })];
    let name_paragraph = Paragraph::new(name_texts.iter())
        .block(Block::default().style(header_base_style))
        .style(header_base_style);
    f.render_widget(name_paragraph, name_area);
}

fn draw_body<B: Backend>(f: &mut Frame<B>, App { stock, .. }: &App, area: Rect) {
    #[allow(clippy::blacklisted_name)]
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
        .unwrap_or(Utc.ymd(1, 1, 1).and_hms(0, 0, 0).timestamp() as f64);
    let max_timestamp = Utc::now().timestamp() as f64;
    let x_axis_bounds = [min_timestamp, max_timestamp];
    let x_axis_labels = [
        Utc.timestamp(min_timestamp as i64, 0)
            .format("%Y-%m-%d")
            .to_string(),
        Utc.timestamp(max_timestamp as i64, 0)
            .format("%Y-%m-%d")
            .to_string(),
    ];

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
        .unwrap_or(f64::INFINITY);
    let price_limit_high = round::ceil(price_limit_high, 0);
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
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .horizontal_margin(if ui_state.time_frame_menu_state.active {
            0
        } else {
            1
        })
        .constraints(vec![Constraint::Min(5), Constraint::Length(20)])
        .split(area);
    let time_frame_area = chunks[1];

    let menu_active_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);

    let time_frame_texts = vec![
        Text::styled(
            "Time frame: ",
            if ui_state.time_frame_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
        Text::styled(
            ui_state.time_frame.to_string(),
            if ui_state.time_frame_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
    ];
    let time_frame_paragraph = Paragraph::new(time_frame_texts.iter())
        .block(if ui_state.time_frame_menu_state.active {
            Block::default()
                .style(if ui_state.time_frame_menu_state.active {
                    menu_active_base_style
                } else {
                    Style::default()
                })
                .borders(Borders::ALL ^ Borders::TOP)
                .border_style(Style::default().fg(Color::Gray))
        } else {
            Block::default()
        })
        .style(if ui_state.time_frame_menu_state.active {
            menu_active_base_style
        } else {
            Style::default()
        })
        .alignment(Alignment::Right);
    f.render_widget(time_frame_paragraph, time_frame_area);

    if ui_state.time_frame_menu_state.active {
        let time_frame_menu_items = TimeFrame::iter().map(|t| Text::raw(t.to_string()));

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Min(5), Constraint::Length(20)])
            .split(f.size());
        let time_frame_menu_area = chunks[1];
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Min(5),
                Constraint::Length(cmp::min(TimeFrame::iter().count() as u16 + 2, 10)),
                Constraint::Length(2),
            ])
            .split(time_frame_menu_area);
        let time_frame_menu_area = chunks[1];

        let time_frame_menu_list = List::new(time_frame_menu_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Gray)),
            )
            .style(Style::default().bg(Color::Reset))
            .highlight_style(Style::default().fg(Color::Black).bg(Color::LightCyan));

        f.render_widget(Clear, time_frame_menu_area);
        f.render_stateful_widget(
            time_frame_menu_list,
            time_frame_menu_area,
            &mut *ui_state.time_frame_menu_state.list_state_write(),
        );
    }
}
