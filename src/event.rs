use crate::{
    app::UiTarget,
    reactive::{Grouped, StreamExt},
    widgets::{SelectMenuState, TextFieldState},
};
use bimap::BiMap;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent};
use derivative::Derivative;
use im::{hashmap, hashmap::HashMap};
use log::debug;
use reactive_rs::Stream;
use std::{cell::RefCell, collections::VecDeque, iter, rc::Rc};
use tui::layout::Rect;

#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Tick,
}

#[derive(Clone, Copy, Debug)]
pub enum ChartEvent {
    PanBackward,
    PanForward,
    Reset,
}

#[derive(Clone, Debug)]
pub enum TextFieldEvent {
    Accept(String),
    Activate,
    BackspacePastStart,
    Deactivate,
    DeletePastEnd,
    Input(String),
    MoveCursor(usize),
    MoveCursorPastEnd,
    MoveCursorPastStart,
    Toggle,
}

#[derive(Clone, Debug)]
pub enum SelectMenuEvent {
    Accept(Option<String>),
    Activate,
    Deactivate,
    SelectIndex(usize),
    Toggle,
}

#[derive(Clone, Debug)]
pub enum OverlayEvent {
    SelectMenu(SelectMenuEvent),
    TextField(TextFieldEvent),
}

#[derive(Clone, Copy, Debug, Derivative, Eq, PartialEq)]
#[derivative(Default)]
pub enum OverlayState {
    Active,
    #[derivative(Default)]
    Inactive,
}

pub fn to_grouped_user_input_events<'a, S, U, R, C>(
    user_input_events: S,
    ui_target_areas: U,
    active_overlays: R,
    hotkey_overlay_map: BiMap<KeyCode, UiTarget>,
    associated_overlay_map: HashMap<UiTarget, UiTarget>,
) -> impl Stream<'a, Item = Grouped<'a, Option<UiTarget>, InputEvent, C>, Context = C>
where
    S: Stream<'a, Item = InputEvent, Context = C>,
    U: Stream<'a, Item = (UiTarget, Option<Rect>)>,
    R: Stream<'a, Item = Option<UiTarget>>,
    C: 'a,
{
    user_input_events
        .with_latest_from(
            ui_target_areas
                .filter({
                    let associated_overlay_map = associated_overlay_map.clone();
                    move |(ui_target, _)| associated_overlay_map.contains_key(ui_target)
                })
                .buffer(associated_overlay_map.len())
                .map(|ui_target_areas| {
                    ui_target_areas
                        .iter()
                        .filter_map(|(ui_target, area)| area.map(|area| (*ui_target, area)))
                        .rev()
                        .collect::<Vec<_>>()
                }),
            |(ev, ui_target_areas)| (*ev, ui_target_areas.clone()),
        )
        .with_latest_from(
            active_overlays,
            |((ev, ui_target_areas), active_overlay)| {
                (*ev, ui_target_areas.clone(), *active_overlay)
            },
        )
        .group_by(
            move |(ev, ui_target_areas, active_overlay)| match *ev {
                InputEvent::Key(KeyEvent { code, .. }) => {
                    let overlay = match active_overlay {
                        Some(ui_target) => Some(*ui_target),
                        None => hotkey_overlay_map.get_by_left(&code).copied(),
                    };
                    debug!("key press grouped into overlay: {:?}", overlay);
                    overlay
                }
                InputEvent::Mouse(MouseEvent::Up(MouseButton::Left, x, y, _)) => {
                    let overlay = ui_target_areas
                        .iter()
                        .find(|(_, area)| {
                            area.left() <= x
                                && area.right() > x
                                && area.top() <= y
                                && area.bottom() > y
                        })
                        .map_or_else(
                            || *active_overlay,
                            |(clicked, _)| associated_overlay_map.get(clicked).copied(),
                        );
                    debug!("mouse click grouped into overlay: {:?}", overlay);
                    overlay
                }
                _ => None,
            },
            |(ev, ..)| *ev,
        )
}

pub fn to_chart_events<'a, S, C>(input_events: S) -> impl Stream<'a, Item = ChartEvent, Context = C>
where
    S: Stream<'a, Item = InputEvent, Context = C>,
    C: 'a + Clone,
{
    input_events.filter_map(|ev| match ev {
        InputEvent::Key(KeyEvent { code, .. }) => match code {
            KeyCode::Left => Some(ChartEvent::PanBackward),
            KeyCode::Right => Some(ChartEvent::PanForward),
            KeyCode::End => Some(ChartEvent::Reset),
            KeyCode::PageUp => Some(ChartEvent::PanBackward),
            KeyCode::PageDown => Some(ChartEvent::PanForward),
            _ => None,
        },
        _ => None,
    })
}

pub fn to_text_field_events<'a, S, O, U, F, C>(
    input_events: S,
    init_text_field_state: TextFieldState,
    overlay_states: O,
    activation_hotkey: KeyCode,
    ui_target_areas: U,
    self_ui_target: UiTarget,
    text_field_event_map: HashMap<Option<UiTarget>, TextFieldEvent>,
    map_value_func: F,
) -> impl Stream<'a, Item = (TextFieldEvent, TextFieldState), Context = C>
where
    S: Stream<'a, Item = InputEvent, Context = C>,
    O: Stream<'a, Item = OverlayState>,
    U: Stream<'a, Item = (UiTarget, Option<Rect>)>,
    F: 'a + Clone + FnOnce(String) -> String,
    C: 'a + Clone,
{
    let text_field_event_map = text_field_event_map.without(&Some(self_ui_target));

    let ui_target_area_bufs = ui_target_areas
        .filter({
            let text_field_event_map = text_field_event_map.clone();
            move |(ui_target, _)| {
                *ui_target == self_ui_target || text_field_event_map.contains_key(&Some(*ui_target))
            }
        })
        .buffer(text_field_event_map.without(&None).len() + 1)
        .map(move |ui_target_areas| {
            ui_target_areas
                .iter()
                .filter_map(|(ui_target, area)| area.map(|area| (*ui_target, area)))
                .rev()
                .collect::<Vec<_>>()
        });

    input_events
        .combine_latest(
            overlay_states.distinct_until_changed(),
            |(ev, overlay_state)| (*ev, *overlay_state),
        )
        .with_latest_from(
            ui_target_area_bufs,
            |((ev, overlay_state), ui_target_areas)| (*ev, *overlay_state, ui_target_areas.clone()),
        )
        .fold(
            (
                None,
                init_text_field_state.clone(),
                init_text_field_state,
                OverlayState::default(),
            ),
            move |(_, acc_text_field_state, acc_saved_text_field_state, acc_overlay_state),
                  (ev, overlay_state, ui_target_areas)| {
                let noop = || {
                    (
                        None,
                        acc_text_field_state.clone(),
                        acc_saved_text_field_state.clone(),
                        *overlay_state,
                    )
                };

                let overlay_state_transitioned = acc_overlay_state != overlay_state;
                if overlay_state_transitioned {
                    let overlay_state_changed = match overlay_state {
                        OverlayState::Active => !acc_text_field_state.active,
                        OverlayState::Inactive => acc_text_field_state.active,
                    };
                    if !overlay_state_changed {
                        return noop();
                    }

                    return match (acc_overlay_state, overlay_state) {
                        (OverlayState::Inactive, OverlayState::Active) => (
                            Some(TextFieldEvent::Activate),
                            TextFieldState {
                                active: true,
                                ..acc_text_field_state.clone()
                            },
                            acc_saved_text_field_state.clone(),
                            *overlay_state,
                        ),
                        (OverlayState::Active, OverlayState::Inactive) => (
                            Some(TextFieldEvent::Deactivate),
                            acc_saved_text_field_state.clone(),
                            acc_saved_text_field_state.clone(),
                            *overlay_state,
                        ),
                        _ => {
                            unreachable!();
                        }
                    };
                }

                match ev {
                    InputEvent::Key(KeyEvent { code, .. }) => match code {
                        KeyCode::Enter
                            if acc_text_field_state.active
                                && !acc_text_field_state.value.is_empty() =>
                        {
                            (
                                Some(TextFieldEvent::Accept(
                                    acc_text_field_state.value.trim().to_owned(),
                                )),
                                acc_saved_text_field_state.clone(),
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Esc if acc_text_field_state.active => (
                            Some(TextFieldEvent::Deactivate),
                            acc_saved_text_field_state.clone(),
                            acc_saved_text_field_state.clone(),
                            *overlay_state,
                        ),
                        KeyCode::Backspace
                            if acc_text_field_state.active
                                && acc_text_field_state.cursor_offset == 0 =>
                        {
                            (
                                Some(TextFieldEvent::BackspacePastStart),
                                acc_text_field_state.clone(),
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Backspace if acc_text_field_state.active => {
                            let mut value = acc_text_field_state.value.clone();
                            let cursor_offset = acc_text_field_state.cursor_offset;
                            debug_assert!(cursor_offset > 0);
                            if cursor_offset == value.chars().count() {
                                value.pop();
                            } else {
                                value = value
                                    .chars()
                                    .take(cursor_offset - 1)
                                    .chain(value.chars().skip(cursor_offset))
                                    .collect();
                            }
                            let map_value_func = map_value_func.clone();
                            let value = map_value_func(value);
                            (
                                Some(TextFieldEvent::Input(value.clone())),
                                TextFieldState {
                                    cursor_offset: cursor_offset - 1,
                                    value,
                                    ..*acc_text_field_state
                                },
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Delete
                            if acc_text_field_state.active
                                && acc_text_field_state.cursor_offset
                                    == acc_text_field_state.value.chars().count() =>
                        {
                            (
                                Some(TextFieldEvent::DeletePastEnd),
                                acc_text_field_state.clone(),
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Delete if acc_text_field_state.active => {
                            let mut value = acc_text_field_state.value.clone();
                            let cursor_offset = acc_text_field_state.cursor_offset;
                            debug_assert!(cursor_offset < value.chars().count());
                            value = value
                                .chars()
                                .take(cursor_offset)
                                .chain(value.chars().skip(cursor_offset + 1))
                                .collect();
                            let map_value_func = map_value_func.clone();
                            let value = map_value_func(value);
                            (
                                Some(TextFieldEvent::Input(value.clone())),
                                TextFieldState {
                                    value,
                                    ..*acc_text_field_state
                                },
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Left
                            if acc_text_field_state.active
                                && acc_text_field_state.cursor_offset == 0 =>
                        {
                            (
                                Some(TextFieldEvent::MoveCursorPastStart),
                                acc_text_field_state.clone(),
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Left if acc_text_field_state.active => {
                            let cursor_offset = acc_text_field_state.cursor_offset;
                            debug_assert!(cursor_offset > 0);
                            (
                                Some(TextFieldEvent::MoveCursor(cursor_offset - 1)),
                                TextFieldState {
                                    cursor_offset: cursor_offset - 1,
                                    ..acc_text_field_state.clone()
                                },
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Right
                            if acc_text_field_state.active
                                && acc_text_field_state.cursor_offset
                                    == acc_text_field_state.value.chars().count() =>
                        {
                            (
                                Some(TextFieldEvent::MoveCursorPastEnd),
                                acc_text_field_state.clone(),
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Right if acc_text_field_state.active => {
                            let cursor_offset = acc_text_field_state.cursor_offset;
                            debug_assert!(
                                cursor_offset < acc_text_field_state.value.chars().count()
                            );
                            (
                                Some(TextFieldEvent::MoveCursor(cursor_offset + 1)),
                                TextFieldState {
                                    cursor_offset: cursor_offset + 1,
                                    ..acc_text_field_state.clone()
                                },
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Home if acc_text_field_state.active => (
                            Some(TextFieldEvent::MoveCursor(0)),
                            TextFieldState {
                                cursor_offset: 0,
                                ..acc_text_field_state.clone()
                            },
                            acc_saved_text_field_state.clone(),
                            *overlay_state,
                        ),
                        KeyCode::End if acc_text_field_state.active => (
                            Some(TextFieldEvent::MoveCursor(
                                acc_text_field_state.value.chars().count(),
                            )),
                            TextFieldState {
                                cursor_offset: acc_text_field_state.value.chars().count(),
                                ..acc_text_field_state.clone()
                            },
                            acc_saved_text_field_state.clone(),
                            *overlay_state,
                        ),
                        &key_code
                            if key_code == activation_hotkey && !acc_text_field_state.active =>
                        {
                            (
                                Some(TextFieldEvent::Activate),
                                TextFieldState {
                                    active: true,
                                    ..acc_text_field_state.clone()
                                },
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Char(c) if acc_text_field_state.active => {
                            let mut value = acc_text_field_state.value.clone();
                            let cursor_offset = acc_text_field_state.cursor_offset;
                            if cursor_offset == value.chars().count() {
                                value.push(*c);
                            } else {
                                value = value
                                    .chars()
                                    .take(cursor_offset)
                                    .chain(iter::once(*c))
                                    .chain(value.chars().skip(cursor_offset))
                                    .collect();
                            }
                            let map_value_func = map_value_func.clone();
                            let value = map_value_func(value);
                            (
                                Some(TextFieldEvent::Input(value.clone())),
                                TextFieldState {
                                    cursor_offset: cursor_offset + 1,
                                    value,
                                    ..*acc_text_field_state
                                },
                                acc_saved_text_field_state.clone(),
                                *overlay_state,
                            )
                        }
                        _ => noop(),
                    },
                    &InputEvent::Mouse(MouseEvent::Up(MouseButton::Left, x, y, _)) => {
                        let _point = (x, y);
                        let hit = ui_target_areas.iter().find(|(_, area)| {
                            area.left() <= x
                                && area.right() > x
                                && area.top() <= y
                                && area.bottom() > y
                        });

                        match hit {
                            Some(&(ui_target, _area))
                                if ui_target == self_ui_target && acc_text_field_state.active =>
                            {
                                noop()
                            }
                            _ => match text_field_event_map
                                .get(&hit.map(|(ui_target, _)| *ui_target))
                            {
                                Some(TextFieldEvent::Activate) if !acc_text_field_state.active => (
                                    Some(TextFieldEvent::Activate),
                                    TextFieldState {
                                        active: true,
                                        ..acc_text_field_state.clone()
                                    },
                                    acc_saved_text_field_state.clone(),
                                    *overlay_state,
                                ),
                                Some(TextFieldEvent::Activate) if acc_text_field_state.active => {
                                    noop()
                                }
                                Some(TextFieldEvent::Deactivate) | Some(TextFieldEvent::Toggle)
                                    if acc_text_field_state.active =>
                                {
                                    (
                                        Some(TextFieldEvent::Deactivate),
                                        acc_saved_text_field_state.clone(),
                                        acc_saved_text_field_state.clone(),
                                        *overlay_state,
                                    )
                                }
                                Some(TextFieldEvent::Deactivate)
                                    if !acc_text_field_state.active =>
                                {
                                    noop()
                                }
                                Some(TextFieldEvent::Toggle) if !acc_text_field_state.active => (
                                    Some(TextFieldEvent::Activate),
                                    TextFieldState {
                                        active: true,
                                        ..acc_text_field_state.clone()
                                    },
                                    acc_saved_text_field_state.clone(),
                                    *overlay_state,
                                ),
                                Some(ev) => {
                                    unimplemented!("unhandled text field event: {:?}", ev);
                                }
                                None => noop(),
                            },
                        }
                    }
                    _ => noop(),
                }
            },
        )
        .filter_map(|(ev, text_field_state, ..)| {
            ev.as_ref()
                .cloned()
                .map(|ev| (ev, text_field_state.clone()))
        })
}

pub fn to_select_menu_events<'a, S, V, O, U, C>(
    input_events: S,
    init_select_menu_state: SelectMenuState<V>,
    overlay_states: O,
    activation_hotkey: KeyCode,
    ui_target_areas: U,
    self_ui_target: UiTarget,
    select_menu_event_map: HashMap<Option<UiTarget>, SelectMenuEvent>,
) -> impl Stream<'a, Item = (SelectMenuEvent, SelectMenuState<V>), Context = C>
where
    S: Stream<'a, Item = InputEvent, Context = C>,
    V: 'a + Clone + PartialEq + ToString,
    O: Stream<'a, Item = OverlayState>,
    U: Stream<'a, Item = (UiTarget, Option<Rect>)>,
    C: 'a + Clone,
{
    let select_menu_event_map = select_menu_event_map.without(&Some(self_ui_target));

    let ui_target_area_bufs = ui_target_areas
        .filter({
            let select_menu_event_map = select_menu_event_map.clone();
            move |(ui_target, _)| {
                *ui_target == self_ui_target
                    || select_menu_event_map.contains_key(&Some(*ui_target))
            }
        })
        .buffer(select_menu_event_map.without(&None).len() + 1)
        .map(move |ui_target_areas| {
            ui_target_areas
                .iter()
                .filter_map(|(ui_target, area)| area.map(|area| (*ui_target, area)))
                .rev()
                .collect::<Vec<_>>()
        });

    input_events
        .combine_latest(
            overlay_states.distinct_until_changed(),
            |(ev, overlay_state)| (*ev, *overlay_state),
        )
        .with_latest_from(
            ui_target_area_bufs,
            |((ev, overlay_state), ui_target_areas)| (*ev, *overlay_state, ui_target_areas.clone()),
        )
        .fold(
            (
                None,
                init_select_menu_state.clone(),
                init_select_menu_state,
                OverlayState::default(),
            ),
            move |(_, acc_select_menu_state, acc_saved_select_menu_state, acc_overlay_state),
                  (ev, overlay_state, ui_target_areas)| {
                let noop = || {
                    (
                        None,
                        acc_select_menu_state.clone(),
                        acc_saved_select_menu_state.clone(),
                        *overlay_state,
                    )
                };

                let overlay_state_transitioned = acc_overlay_state != overlay_state;
                if overlay_state_transitioned {
                    let overlay_state_changed = match overlay_state {
                        OverlayState::Active => !acc_select_menu_state.active,
                        OverlayState::Inactive => acc_select_menu_state.active,
                    };
                    if !overlay_state_changed {
                        return noop();
                    }

                    return match (acc_overlay_state, overlay_state) {
                        (OverlayState::Inactive, OverlayState::Active) => (
                            Some(SelectMenuEvent::Activate),
                            {
                                let mut select_menu_state = acc_saved_select_menu_state.clone();
                                select_menu_state.active = true;
                                select_menu_state
                            },
                            acc_saved_select_menu_state.clone(),
                            *overlay_state,
                        ),
                        (OverlayState::Active, OverlayState::Inactive) => (
                            Some(SelectMenuEvent::Deactivate),
                            acc_saved_select_menu_state.clone(),
                            acc_saved_select_menu_state.clone(),
                            *overlay_state,
                        ),
                        _ => {
                            unreachable!();
                        }
                    };
                }

                match ev {
                    InputEvent::Key(KeyEvent { code, .. }) => match code {
                        KeyCode::Enter if acc_select_menu_state.active => {
                            let select_menu_state = {
                                let mut select_menu_state = acc_select_menu_state.clone();
                                select_menu_state.active = false;
                                select_menu_state
                            };
                            (
                                Some(SelectMenuEvent::Accept(
                                    select_menu_state.selected().map(|s| s.to_string()),
                                )),
                                select_menu_state.clone(),
                                select_menu_state,
                                *overlay_state,
                            )
                        }
                        KeyCode::Esc if acc_select_menu_state.active => (
                            Some(SelectMenuEvent::Deactivate),
                            acc_saved_select_menu_state.clone(),
                            acc_saved_select_menu_state.clone(),
                            *overlay_state,
                        ),
                        KeyCode::Up if acc_select_menu_state.active => {
                            let select_menu_state = {
                                let mut select_menu_state = acc_select_menu_state.clone();
                                select_menu_state.select_prev().unwrap();
                                select_menu_state
                            };
                            (
                                Some(SelectMenuEvent::SelectIndex(
                                    select_menu_state.selected_index().unwrap(),
                                )),
                                select_menu_state,
                                acc_saved_select_menu_state.clone(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Down if acc_select_menu_state.active => {
                            let select_menu_state = {
                                let mut select_menu_state = acc_select_menu_state.clone();
                                select_menu_state.select_next().unwrap();
                                select_menu_state
                            };
                            (
                                Some(SelectMenuEvent::SelectIndex(
                                    select_menu_state.selected_index().unwrap(),
                                )),
                                select_menu_state,
                                acc_saved_select_menu_state.clone(),
                                *overlay_state,
                            )
                        }
                        &key_code
                            if key_code == activation_hotkey && !acc_select_menu_state.active =>
                        {
                            (
                                Some(SelectMenuEvent::Activate),
                                {
                                    let mut select_menu_state = acc_saved_select_menu_state.clone();
                                    select_menu_state.active = true;
                                    select_menu_state
                                },
                                acc_saved_select_menu_state.clone(),
                                *overlay_state,
                            )
                        }
                        // KeyCode::Char(c) if acc_select_menu_state.active => {
                        //     todo!();
                        // }
                        _ => (
                            None,
                            acc_select_menu_state.clone(),
                            acc_saved_select_menu_state.clone(),
                            *overlay_state,
                        ),
                    },
                    &InputEvent::Mouse(MouseEvent::Up(MouseButton::Left, x, y, _)) => {
                        let point = (x, y);
                        let hit = ui_target_areas.iter().find(|(_, area)| {
                            area.left() <= x
                                && area.right() > x
                                && area.top() <= y
                                && area.bottom() > y
                        });

                        match hit {
                            Some(&(ui_target, area))
                                if ui_target == self_ui_target && acc_select_menu_state.active =>
                            {
                                if let Some(n) = acc_select_menu_state.point_to_index(area, point) {
                                    let select_menu_state = {
                                        let mut select_menu_state = acc_select_menu_state.clone();
                                        select_menu_state.select_index(n).unwrap();
                                        select_menu_state.active = false;
                                        select_menu_state
                                    };
                                    (
                                        Some(SelectMenuEvent::Accept(
                                            select_menu_state.selected().map(|s| s.to_string()),
                                        )),
                                        select_menu_state.clone(),
                                        select_menu_state,
                                        *overlay_state,
                                    )
                                } else {
                                    noop()
                                }
                            }
                            _ => match select_menu_event_map
                                .get(&hit.map(|(ui_target, _)| *ui_target))
                            {
                                Some(SelectMenuEvent::Activate)
                                    if !acc_select_menu_state.active =>
                                {
                                    (
                                        Some(SelectMenuEvent::Activate),
                                        {
                                            let mut select_menu_state =
                                                acc_saved_select_menu_state.clone();
                                            select_menu_state.active = true;
                                            select_menu_state
                                        },
                                        acc_saved_select_menu_state.clone(),
                                        *overlay_state,
                                    )
                                }
                                Some(SelectMenuEvent::Activate) if acc_select_menu_state.active => {
                                    noop()
                                }
                                Some(SelectMenuEvent::Deactivate)
                                | Some(SelectMenuEvent::Toggle)
                                    if acc_select_menu_state.active =>
                                {
                                    (
                                        Some(SelectMenuEvent::Deactivate),
                                        acc_saved_select_menu_state.clone(),
                                        acc_saved_select_menu_state.clone(),
                                        *overlay_state,
                                    )
                                }
                                Some(SelectMenuEvent::Deactivate)
                                    if !acc_select_menu_state.active =>
                                {
                                    noop()
                                }
                                Some(SelectMenuEvent::Toggle) if !acc_select_menu_state.active => (
                                    Some(SelectMenuEvent::Activate),
                                    {
                                        let mut select_menu_state =
                                            acc_saved_select_menu_state.clone();
                                        select_menu_state.active = true;
                                        select_menu_state
                                    },
                                    acc_saved_select_menu_state.clone(),
                                    *overlay_state,
                                ),
                                Some(ev) => {
                                    unimplemented!("unhandled select menu event: {:?}", ev);
                                }
                                None => noop(),
                            },
                        }
                    }
                    _ => noop(),
                }
            },
        )
        .filter_map(|(ev, select_menu_state, ..)| {
            ev.as_ref()
                .cloned()
                .map(|ev| (ev, select_menu_state.clone()))
        })
}

/// Queues the overlay states to send on next tick.
///
/// This is necessary to prevent a cycle.
pub fn queue_overlay_states_for_next_tick<'a, S>(
    overlay_events: S,
    overlay_state_queue: Rc<RefCell<VecDeque<(UiTarget, OverlayState)>>>,
) where
    S: Stream<'a, Item = (UiTarget, OverlayEvent)>,
{
    overlay_events
        .fold(
            (hashmap! {}, hashmap! {}),
            |(_, acc_overlay_state_map), (ui_target, ev)| {
                let acc_overlay_state = acc_overlay_state_map
                    .get(ui_target)
                    .copied()
                    .unwrap_or(OverlayState::Inactive);

                let overlay_state = match ev {
                    OverlayEvent::TextField(ev) => match ev {
                        TextFieldEvent::Activate => OverlayState::Active,
                        TextFieldEvent::Accept(_) | TextFieldEvent::Deactivate => {
                            OverlayState::Inactive
                        }
                        TextFieldEvent::Toggle if acc_overlay_state == OverlayState::Active => {
                            OverlayState::Inactive
                        }
                        TextFieldEvent::Toggle if acc_overlay_state == OverlayState::Inactive => {
                            OverlayState::Active
                        }
                        _ => acc_overlay_state,
                    },
                    OverlayEvent::SelectMenu(ev) => match ev {
                        SelectMenuEvent::Activate => OverlayState::Active,
                        SelectMenuEvent::Accept(_) | SelectMenuEvent::Deactivate => {
                            OverlayState::Inactive
                        }
                        SelectMenuEvent::Toggle if acc_overlay_state == OverlayState::Active => {
                            OverlayState::Inactive
                        }
                        SelectMenuEvent::Toggle if acc_overlay_state == OverlayState::Inactive => {
                            OverlayState::Active
                        }
                        _ => acc_overlay_state,
                    },
                };

                let overlay_state_map = (hashmap! {*ui_target => overlay_state})
                    + match overlay_state {
                        OverlayState::Active => {
                            acc_overlay_state_map
                                .iter()
                                .filter_map(|(ui_target, overlay_state)| match overlay_state {
                                    OverlayState::Active => {
                                        Some((*ui_target, OverlayState::Inactive))
                                    }
                                    OverlayState::Inactive => None,
                                })
                                .collect::<HashMap<UiTarget, OverlayState>>()
                                + acc_overlay_state_map.clone()
                        }
                        OverlayState::Inactive => acc_overlay_state_map.clone(),
                    };

                let overlay_state_changeset = acc_overlay_state_map.clone().difference_with(
                    overlay_state_map.clone(),
                    |acc_overlay_state, overlay_state| {
                        if acc_overlay_state != overlay_state {
                            Some(overlay_state)
                        } else {
                            None
                        }
                    },
                );

                (overlay_state_changeset, overlay_state_map)
            },
        )
        .subscribe(move |(overlay_state_changeset, ..)| {
            for (ui_target, overlay_state) in overlay_state_changeset {
                debug!("queuing overlay state: {:?}", (ui_target, overlay_state));
                overlay_state_queue
                    .borrow_mut()
                    .push_back((*ui_target, *overlay_state));
            }
        });
}

pub fn to_active_overlays<'a, S, C>(
    overlay_states: S,
) -> impl Stream<'a, Item = Option<UiTarget>, Context = C>
where
    S: Stream<'a, Item = (UiTarget, OverlayState), Context = C>,
{
    overlay_states
        .fold(
            None,
            |acc_active_overlay, (ui_target, overlay_state)| match overlay_state {
                OverlayState::Active => Some(*ui_target),
                OverlayState::Inactive => {
                    if acc_active_overlay.as_ref() == Some(ui_target) {
                        None
                    } else {
                        *acc_active_overlay
                    }
                }
            },
        )
        .distinct_until_changed()
        .inspect(|active_overlay| {
            debug!("active overlay: {:?}", active_overlay);
        })
}
