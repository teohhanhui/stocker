use crate::app::{App, TimeFrame, UiState, UiTarget};
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
    draw_overlay(f, app);
}

fn draw_header<B: Backend>(
    f: &mut Frame<B>,
    App {
        stock,
        ui_state: UiState { target_areas, .. },
        ..
    }: &App,
    area: Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .horizontal_margin(1)
        .constraints(vec![Constraint::Length(10), Constraint::Length(20)])
        .split(area);
    let stock_symbol_area = chunks[0];
    let stock_name_area = chunks[1];

    let header_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);

    let header_block = Block::default().style(header_base_style);
    f.render_widget(header_block, area);

    let stock_symbol_texts = vec![Text::raw(stock.symbol.as_str())];
    let stock_symbol_paragraph = Paragraph::new(stock_symbol_texts.iter())
        .block(Block::default().style(header_base_style))
        .style(header_base_style.clone().modifier(Modifier::BOLD));
    f.render_widget(stock_symbol_paragraph, stock_symbol_area);

    target_areas
        .write()
        .insert(UiTarget::StockSymbol, stock_symbol_area);

    let stock_name_texts = vec![Text::raw(match &stock.profile {
        Some(Profile::Company(company)) => company.name.as_str(),
        Some(Profile::Fund(fund)) => fund.name.as_str(),
        None => "",
    })];
    let stock_name_paragraph = Paragraph::new(stock_name_texts.iter())
        .block(Block::default().style(header_base_style))
        .style(header_base_style);
    f.render_widget(stock_name_paragraph, stock_name_area);

    target_areas
        .write()
        .insert(UiTarget::StockName, stock_name_area);
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
        .style(Style::default().fg({
            let (_, first_price) = historical_prices_data.first().unwrap_or(&(0f64, 0f64));
            let (_, last_price) = historical_prices_data.last().unwrap_or(&(0f64, 0f64));
            if last_price >= first_price {
                Color::Green
            } else {
                Color::Red
            }
        }))
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

fn draw_footer<B: Backend>(
    f: &mut Frame<B>,
    App {
        ui_state:
            UiState {
                target_areas,
                time_frame,
                time_frame_menu_state,
                ..
            },
        ..
    }: &App,
    area: Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .horizontal_margin(if time_frame_menu_state.active { 0 } else { 1 })
        .constraints(vec![Constraint::Min(5), Constraint::Length(20)])
        .split(area);
    let time_frame_area = chunks[1];

    let menu_active_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);

    let time_frame_texts = vec![
        Text::styled(
            "Time frame: ",
            if time_frame_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
        Text::styled(
            time_frame.to_string(),
            if time_frame_menu_state.active {
                menu_active_base_style
            } else {
                Style::default()
            },
        ),
    ];
    let time_frame_paragraph = Paragraph::new(time_frame_texts.iter())
        .block(if time_frame_menu_state.active {
            Block::default()
                .style(if time_frame_menu_state.active {
                    menu_active_base_style
                } else {
                    Style::default()
                })
                .borders(Borders::ALL ^ Borders::TOP)
                .border_style(Style::default().fg(Color::Gray))
        } else {
            Block::default()
        })
        .style(if time_frame_menu_state.active {
            menu_active_base_style
        } else {
            Style::default()
        })
        .alignment(Alignment::Right);
    f.render_widget(time_frame_paragraph, time_frame_area);

    target_areas
        .write()
        .insert(UiTarget::TimeFrame, time_frame_area);
}

fn draw_overlay<B: Backend>(
    f: &mut Frame<B>,
    App {
        ui_state:
            UiState {
                stock_symbol_input_state,
                target_areas,
                time_frame_menu_state,
                ..
            },
        ..
    }: &App,
) {
    let active_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);
    let highlight_base_style = Style::default().fg(Color::Black).bg(Color::White);

    if stock_symbol_input_state.active {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Length(30), Constraint::Min(5)])
            .split(f.size());
        let stock_symbol_input_area = chunks[0];
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(5),
            ])
            .split(stock_symbol_input_area);
        let stock_symbol_input_area = chunks[1];

        let stock_symbol_input_texts = vec![Text::raw(stock_symbol_input_state.value.as_str())];
        let stock_symbol_input_paragraph = Paragraph::new(stock_symbol_input_texts.iter())
            .block(
                Block::default()
                    .style(active_base_style)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Gray)),
            )
            .style(active_base_style);
        f.render_widget(Clear, stock_symbol_input_area);
        f.render_widget(stock_symbol_input_paragraph, stock_symbol_input_area);

        target_areas
            .write()
            .insert(UiTarget::StockSymbolInput, stock_symbol_input_area);
    }

    if time_frame_menu_state.active {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Min(5), Constraint::Length(20)])
            .split(f.size());
        let time_frame_menu_area = chunks[1];
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Min(5),
                Constraint::Length(cmp::min(TimeFrame::iter().count() as u16 + 2, 12)),
                Constraint::Length(2),
            ])
            .split(time_frame_menu_area);
        let time_frame_menu_area = chunks[1];

        let time_frame_menu_items = TimeFrame::iter().map(|t| Text::raw(t.to_string()));
        let time_frame_menu_list = List::new(time_frame_menu_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Gray)),
            )
            .highlight_style(highlight_base_style);

        f.render_widget(Clear, time_frame_menu_area);
        f.render_stateful_widget(
            time_frame_menu_list,
            time_frame_menu_area,
            &mut *time_frame_menu_state.list_state_write(),
        );

        target_areas
            .write()
            .insert(UiTarget::TimeFrameMenu, time_frame_menu_area);
    }
}
