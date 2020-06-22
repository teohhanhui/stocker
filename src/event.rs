use crate::{
    app::{InputState, UiTarget},
    reactive::StreamExt,
};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent};
use im::hashmap::HashMap;
use reactive_rs::Stream;
use tui::layout::Rect;

#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Tick,
}

#[derive(Clone, Debug)]
pub enum TextFieldEvent {
    Accept(String),
    Activate,
    Cancel,
    Modify(String),
    Toggle,
}

pub fn input_events_to_text_field_events<'a, S, R, F, C>(
    input_events: S,
    activation_key_code: KeyCode,
    map_mouse_funcs: R,
    map_value_func: F,
) -> impl Stream<'a, Item = TextFieldEvent, Context = C>
where
    S: Stream<'a, Item = InputEvent, Context = C>,
    R: Stream<'a, Item = TextFieldMapMouseFn>,
    F: 'a + Clone + FnOnce(String) -> String,
{
    input_events
        .with_latest_from(map_mouse_funcs, |(ev, map_mouse_func)| {
            (*ev, map_mouse_func.clone())
        })
        .fold(
            (InputState::default(), None),
            move |(acc_input_state, _), (ev, map_mouse_func)| match ev {
                InputEvent::Key(KeyEvent { code, .. }) => match code {
                    KeyCode::Backspace if acc_input_state.active => {
                        let mut value = acc_input_state.value.clone();
                        value.pop();
                        let map_value_func = map_value_func.clone();
                        let value = map_value_func(value);
                        (
                            InputState {
                                value: value.clone(),
                                ..*acc_input_state
                            },
                            Some(TextFieldEvent::Modify(value)),
                        )
                    }
                    KeyCode::Enter
                        if acc_input_state.active && !acc_input_state.value.is_empty() =>
                    {
                        (
                            InputState::default(),
                            Some(TextFieldEvent::Accept(
                                acc_input_state.value.trim().to_owned(),
                            )),
                        )
                    }
                    KeyCode::Esc if acc_input_state.active => {
                        (InputState::default(), Some(TextFieldEvent::Cancel))
                    }
                    &key_code if key_code == activation_key_code && !acc_input_state.active => (
                        InputState {
                            active: true,
                            value: acc_input_state.value.clone(),
                        },
                        Some(TextFieldEvent::Activate),
                    ),
                    KeyCode::Char(c) if acc_input_state.active => {
                        let mut value = acc_input_state.value.clone();
                        value.push(*c);
                        let map_value_func = map_value_func.clone();
                        let value = map_value_func(value);
                        (
                            InputState {
                                value: value.clone(),
                                ..*acc_input_state
                            },
                            Some(TextFieldEvent::Modify(value)),
                        )
                    }
                    _ => (acc_input_state.clone(), None),
                },
                InputEvent::Mouse(MouseEvent::Up(MouseButton::Left, x, y, _)) => {
                    map_mouse_func.call(acc_input_state.clone(), (*x, *y))
                }
                _ => (acc_input_state.clone(), None),
            },
        )
        .filter_map(|(_, ev)| ev.clone())
}

pub fn text_field_events_to_input_states<'a, S, C>(
    text_field_events: S,
) -> impl Stream<'a, Item = InputState, Context = C>
where
    S: Stream<'a, Item = TextFieldEvent, Context = C>,
    C: Default,
{
    text_field_events
        .fold(InputState::default(), |acc_input_state, ev| match ev {
            TextFieldEvent::Activate => InputState {
                active: true,
                value: acc_input_state.value.clone(),
            },
            TextFieldEvent::Modify(value) => InputState {
                value: value.clone(),
                ..*acc_input_state
            },
            TextFieldEvent::Accept(_) | TextFieldEvent::Cancel => InputState::default(),
            TextFieldEvent::Toggle if acc_input_state.active => InputState::default(),
            TextFieldEvent::Toggle if !acc_input_state.active => InputState {
                active: true,
                value: acc_input_state.value.clone(),
            },
            _ => {
                unreachable!();
            }
        })
        .start_with(InputState::default())
}

pub fn ui_target_areas_to_text_field_map_mouse_funcs<'a, S, C>(
    ui_target_areas: S,
    text_field_event_map: HashMap<Option<UiTarget>, Option<TextFieldEvent>>,
) -> impl Stream<'a, Item = TextFieldMapMouseFn, Context = C>
where
    S: Stream<'a, Item = (UiTarget, Option<Rect>), Context = C>,
{
    ui_target_areas
        .filter({
            let text_field_event_map = text_field_event_map.clone();
            move |(ui_target, _)| text_field_event_map.contains_key(&Some(*ui_target))
        })
        .buffer(text_field_event_map.without(&None).len())
        .map(move |ui_target_areas| {
            let ui_target_areas: Vec<_> = ui_target_areas
                .iter()
                .filter_map(|(ui_target, area)| {
                    if let Some(area) = area {
                        Some((*ui_target, *area))
                    } else {
                        None
                    }
                })
                .rev()
                .collect();
            let text_field_event_map = text_field_event_map.clone();

            TextFieldMapMouseFn {
                text_field_event_map,
                ui_target_areas,
            }
        })
}

#[derive(Clone, Debug)]
pub struct TextFieldMapMouseFn {
    text_field_event_map: HashMap<Option<UiTarget>, Option<TextFieldEvent>>,
    /// available UI targets and their respective area, in reverse z-order (top-most to bottom-most)
    ui_target_areas: Vec<(UiTarget, Rect)>,
}

impl TextFieldMapMouseFn {
    pub fn call(
        &self,
        input_state: InputState,
        (x, y): (u16, u16),
    ) -> (InputState, Option<TextFieldEvent>) {
        let ui_target_areas = self.ui_target_areas.clone();
        let text_field_event_map = self.text_field_event_map.clone();

        let hit_target = ui_target_areas
            .into_iter()
            .find(|(_, area)| {
                area.left() <= x && area.right() > x && area.top() <= y && area.bottom() > y
            })
            .map(|(hit_target, _)| hit_target);

        let active = input_state.active;

        match text_field_event_map.get(&hit_target).unwrap() {
            Some(TextFieldEvent::Activate) if !active => (
                InputState {
                    active: true,
                    ..input_state
                },
                Some(TextFieldEvent::Activate),
            ),
            Some(TextFieldEvent::Modify(_)) => {
                unimplemented!();
            }
            Some(TextFieldEvent::Accept(_)) => {
                unimplemented!();
            }
            Some(TextFieldEvent::Cancel) | Some(TextFieldEvent::Toggle) if active => {
                (InputState::default(), Some(TextFieldEvent::Cancel))
            }
            Some(TextFieldEvent::Toggle) if !active => (
                InputState {
                    active: true,
                    ..input_state
                },
                Some(TextFieldEvent::Activate),
            ),
            _ => (input_state, None),
        }
    }
}
