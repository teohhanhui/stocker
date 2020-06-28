use crate::{
    app::{InputState, UiTarget},
    reactive::{Grouped, StreamExt},
};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent};
use derivative::Derivative;
use im::{hashmap, hashmap::HashMap};
use log::debug;
use reactive_rs::Stream;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
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
    Deactivate,
    Input(String),
    MoveCursorBackward,
    MoveCursorForward,
    MoveCursorTo(usize),
    Toggle,
}

#[derive(Clone, Copy, Debug)]
pub enum SelectMenuEvent {
    Accept(usize),
    Activate,
    Deactivate,
    SelectNext,
    SelectNth(usize),
    SelectPrev,
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
    hotkey_overlay_map: HashMap<KeyCode, UiTarget>,
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
                        None => hotkey_overlay_map.get(&code).copied(),
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

pub fn to_text_field_events<'a, S, U, R, F, C>(
    input_events: S,
    overlay_states: U,
    activation_hotkey: KeyCode,
    map_mouse_funcs: R,
    map_value_func: F,
) -> impl Stream<'a, Item = TextFieldEvent, Context = C>
where
    S: Stream<'a, Item = InputEvent, Context = C>,
    U: Stream<'a, Item = OverlayState>,
    R: Stream<'a, Item = ToTextFieldMapMouseFn>,
    F: 'a + Clone + FnOnce(String) -> String,
    C: 'a + Clone,
{
    input_events
        .combine_latest(
            overlay_states.distinct_until_changed(),
            |(ev, overlay_state)| (*ev, *overlay_state),
        )
        .with_latest_from(map_mouse_funcs, |((ev, overlay_state), map_mouse_func)| {
            (*ev, *overlay_state, map_mouse_func.clone())
        })
        .fold(
            (None, InputState::default(), OverlayState::default()),
            move |(_, acc_input_state, acc_overlay_state), (ev, overlay_state, map_mouse_func)| {
                let overlay_state_transitioned = acc_overlay_state != overlay_state;
                if overlay_state_transitioned {
                    let overlay_state_changed = match overlay_state {
                        OverlayState::Active => !acc_input_state.active,
                        OverlayState::Inactive => acc_input_state.active,
                    };
                    if !overlay_state_changed {
                        return (None, acc_input_state.clone(), *overlay_state);
                    }

                    return match (acc_overlay_state, overlay_state) {
                        (OverlayState::Inactive, OverlayState::Active) => (
                            Some(TextFieldEvent::Activate),
                            InputState {
                                active: true,
                                value: acc_input_state.value.clone(),
                            },
                            *overlay_state,
                        ),
                        (OverlayState::Active, OverlayState::Inactive) => (
                            Some(TextFieldEvent::Deactivate),
                            InputState::default(),
                            *overlay_state,
                        ),
                        _ => {
                            unreachable!();
                        }
                    };
                }

                match ev {
                    InputEvent::Key(KeyEvent { code, .. }) => match code {
                        KeyCode::Backspace if acc_input_state.active => {
                            let mut value = acc_input_state.value.clone();
                            value.pop();
                            let map_value_func = map_value_func.clone();
                            let value = map_value_func(value);
                            (
                                Some(TextFieldEvent::Input(value.clone())),
                                InputState {
                                    value,
                                    ..*acc_input_state
                                },
                                *overlay_state,
                            )
                        }
                        KeyCode::Enter
                            if acc_input_state.active && !acc_input_state.value.is_empty() =>
                        {
                            (
                                Some(TextFieldEvent::Accept(
                                    acc_input_state.value.trim().to_owned(),
                                )),
                                InputState::default(),
                                *overlay_state,
                            )
                        }
                        KeyCode::Esc if acc_input_state.active => (
                            Some(TextFieldEvent::Deactivate),
                            InputState::default(),
                            *overlay_state,
                        ),
                        &key_code if key_code == activation_hotkey && !acc_input_state.active => (
                            Some(TextFieldEvent::Activate),
                            InputState {
                                active: true,
                                value: acc_input_state.value.clone(),
                            },
                            *overlay_state,
                        ),
                        KeyCode::Char(c) if acc_input_state.active => {
                            let mut value = acc_input_state.value.clone();
                            value.push(*c);
                            let map_value_func = map_value_func.clone();
                            let value = map_value_func(value);
                            (
                                Some(TextFieldEvent::Input(value.clone())),
                                InputState {
                                    value,
                                    ..*acc_input_state
                                },
                                *overlay_state,
                            )
                        }
                        _ => (None, acc_input_state.clone(), *overlay_state),
                    },
                    InputEvent::Mouse(MouseEvent::Up(MouseButton::Left, x, y, _)) => {
                        let (input_state, ev) =
                            map_mouse_func.call(acc_input_state.clone(), (*x, *y));
                        (ev, input_state, *overlay_state)
                    }
                    _ => (None, acc_input_state.clone(), *overlay_state),
                }
            },
        )
        .filter_map(|(ev, ..)| ev.clone())
}

pub fn to_text_field_states<'a, S, C>(
    text_field_events: S,
) -> impl Stream<'a, Item = InputState, Context = C>
where
    S: Stream<'a, Item = TextFieldEvent, Context = C>,
{
    text_field_events.fold(InputState::default(), |acc_input_state, ev| match ev {
        TextFieldEvent::Activate => InputState {
            active: true,
            value: acc_input_state.value.clone(),
        },
        TextFieldEvent::Input(value) => InputState {
            value: value.clone(),
            ..*acc_input_state
        },
        TextFieldEvent::Accept(_) | TextFieldEvent::Deactivate => InputState::default(),
        TextFieldEvent::Toggle if acc_input_state.active => InputState::default(),
        TextFieldEvent::Toggle if !acc_input_state.active => InputState {
            active: true,
            value: acc_input_state.value.clone(),
        },
        _ => {
            unreachable!();
        }
    })
}

/// Collects the overlay states to send on next tick.
///
/// This is necessary to prevent a cycle.
pub fn collect_overlay_states_for_next_tick<'a, S>(
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
            for (ui_target, overlay_state) in overlay_state_changeset.iter() {
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

pub fn to_text_field_map_mouse_funcs<'a, S, C>(
    ui_target_areas: S,
    self_ui_target: UiTarget,
    text_field_event_map: HashMap<Option<UiTarget>, TextFieldEvent>,
) -> impl Stream<'a, Item = ToTextFieldMapMouseFn, Context = C>
where
    S: Stream<'a, Item = (UiTarget, Option<Rect>), Context = C>,
{
    let text_field_event_map = text_field_event_map.without(&Some(self_ui_target));

    ui_target_areas
        .filter({
            let text_field_event_map = text_field_event_map.clone();
            move |(ui_target, _)| {
                *ui_target == self_ui_target || text_field_event_map.contains_key(&Some(*ui_target))
            }
        })
        .buffer(text_field_event_map.without(&None).len() + 1)
        .map(move |ui_target_areas| {
            let ui_target_areas: Vec<_> = ui_target_areas
                .iter()
                .filter_map(|(ui_target, area)| area.map(|area| (*ui_target, area)))
                .rev()
                .collect();

            ToTextFieldMapMouseFn {
                self_ui_target,
                text_field_event_map: text_field_event_map.clone(),
                ui_target_areas,
            }
        })
}

#[derive(Clone, Debug)]
pub struct ToTextFieldMapMouseFn {
    self_ui_target: UiTarget,
    text_field_event_map: HashMap<Option<UiTarget>, TextFieldEvent>,
    /// available UI targets and their respective area, in reverse z-order (top-most to bottom-most)
    ui_target_areas: Vec<(UiTarget, Rect)>,
}

impl ToTextFieldMapMouseFn {
    pub fn call(
        &self,
        input_state: InputState,
        (x, y): (u16, u16),
    ) -> (InputState, Option<TextFieldEvent>) {
        let ui_target_areas = self.ui_target_areas.clone();
        let self_ui_target = self.self_ui_target;
        let text_field_event_map = self.text_field_event_map.clone();

        let hit_target = ui_target_areas
            .iter()
            .find(|(_, area)| {
                area.left() <= x && area.right() > x && area.top() <= y && area.bottom() > y
            })
            .map(|(hit_target, _)| hit_target)
            .copied();

        let active = input_state.active;

        if hit_target == Some(self_ui_target) {
            return (input_state, None);
        }

        match text_field_event_map.get(&hit_target) {
            Some(TextFieldEvent::Activate) if !active => (
                InputState {
                    active: true,
                    ..input_state
                },
                Some(TextFieldEvent::Activate),
            ),
            Some(TextFieldEvent::Activate) if active => (input_state, None),
            Some(TextFieldEvent::Deactivate) | Some(TextFieldEvent::Toggle) if active => {
                (InputState::default(), Some(TextFieldEvent::Deactivate))
            }
            Some(TextFieldEvent::Deactivate) if !active => (input_state, None),
            Some(TextFieldEvent::Toggle) if !active => (
                InputState {
                    active: true,
                    ..input_state
                },
                Some(TextFieldEvent::Activate),
            ),
            Some(ev) => {
                unimplemented!("unhandled text field event: {:?}", ev);
            }
            None => (input_state, None),
        }
    }
}
