use std::collections::VecDeque;
use std::default::Default;
use std::ops::Range;

use x11rb::protocol::{Event, xproto};
use xim::{
    AHashMap, AttributeName, CaretDirection, CaretStyle, Client, ClientError, ClientHandler,
    InputStyle, PreeditDrawStatus,
};

#[derive(Debug)]
pub enum XimCallbackEvent {
    XimXEvent(x11rb::protocol::Event),
    XimPreeditEvent(xproto::Window, String, Option<Range<usize>>),
    XimPreeditDoneEvent(xproto::Window),
    XimCommitEvent(xproto::Window, String),
}

pub struct XimHandler {
    pub im_id: u16,
    pub ic_id: u16,
    pub connected: bool,
    pub client_window_bound: bool,
    pub window: xproto::Window,
    pub callback_events: VecDeque<XimCallbackEvent>,
    preedit_buffer: String,
    preedit_caret: usize,
}

impl XimHandler {
    pub fn new() -> Self {
        Self {
            im_id: Default::default(),
            ic_id: Default::default(),
            connected: false,
            client_window_bound: false,
            window: Default::default(),
            callback_events: VecDeque::new(),
            preedit_buffer: String::new(),
            preedit_caret: 0,
        }
    }

    pub fn clear_preedit(&mut self) {
        self.preedit_buffer.clear();
        self.preedit_caret = 0;
    }

    fn preedit_cursor_utf16(&self) -> Option<Range<usize>> {
        if self.preedit_buffer.is_empty() {
            return None;
        }
        let utf16 = char_index_to_utf16(&self.preedit_buffer, self.preedit_caret);
        Some(utf16..utf16)
    }

    fn apply_commit(&mut self, text: &str) {
        self.clear_preedit();
        self.callback_events
            .push_back(XimCallbackEvent::XimCommitEvent(
                self.window,
                String::from(text),
            ));
    }

    fn apply_preedit_start(&mut self) {
        self.clear_preedit();
    }

    fn apply_preedit_draw(
        &mut self,
        caret: i32,
        chg_first: i32,
        chg_len: i32,
        status: PreeditDrawStatus,
        preedit_string: &str,
    ) {
        let mut char_offsets: Vec<usize> = self
            .preedit_buffer
            .char_indices()
            .map(|(byte_idx, _)| byte_idx)
            .collect();
        char_offsets.push(self.preedit_buffer.len());
        let char_count = char_offsets.len() - 1;

        let first = usize::try_from(chg_first).unwrap_or(0).min(char_count);
        let len = usize::try_from(chg_len)
            .unwrap_or(0)
            .min(char_count - first);
        let replacement = if status.contains(PreeditDrawStatus::NO_STRING) {
            ""
        } else {
            preedit_string
        };

        let start_byte = char_offsets[first];
        let end_byte = char_offsets[first + len];
        self.preedit_buffer
            .replace_range(start_byte..end_byte, replacement);

        let new_char_count = self.preedit_buffer.chars().count();
        self.preedit_caret = usize::try_from(caret).unwrap_or(0).min(new_char_count);
        let cursor = self.preedit_cursor_utf16();

        self.callback_events
            .push_back(XimCallbackEvent::XimPreeditEvent(
                self.window,
                self.preedit_buffer.clone(),
                cursor,
            ));
    }

    fn apply_preedit_caret(&mut self, position: &mut i32, direction: CaretDirection) {
        let char_count = self.preedit_buffer.chars().count();
        match direction {
            CaretDirection::ForwardChar => {
                self.preedit_caret = self.preedit_caret.saturating_add(1).min(char_count);
            }
            CaretDirection::BackwardChar => {
                self.preedit_caret = self.preedit_caret.saturating_sub(1);
            }
            CaretDirection::LineStart => {
                self.preedit_caret = 0;
            }
            CaretDirection::LineEnd => {
                self.preedit_caret = char_count;
            }
            CaretDirection::AbsolutePosition => {
                self.preedit_caret = usize::try_from(*position).unwrap_or(0).min(char_count);
            }
            _ => {
                self.preedit_caret = self.preedit_caret.min(char_count);
            }
        }
        *position = i32::try_from(self.preedit_caret).unwrap_or(i32::MAX);
        if !self.preedit_buffer.is_empty() {
            let cursor = self.preedit_cursor_utf16();
            self.callback_events
                .push_back(XimCallbackEvent::XimPreeditEvent(
                    self.window,
                    self.preedit_buffer.clone(),
                    cursor,
                ));
        }
    }

    fn apply_preedit_done(&mut self) {
        self.clear_preedit();
        self.callback_events
            .push_back(XimCallbackEvent::XimPreeditDoneEvent(self.window));
    }
}

fn char_index_to_utf16(text: &str, char_index: usize) -> usize {
    text.chars().take(char_index).map(char::len_utf16).sum()
}

impl<C: Client<XEvent = xproto::KeyPressEvent>> ClientHandler<C> for XimHandler {
    fn handle_connect(&mut self, client: &mut C) -> Result<(), ClientError> {
        client.open("C")
    }

    fn handle_open(&mut self, client: &mut C, input_method_id: u16) -> Result<(), ClientError> {
        self.im_id = input_method_id;

        client.get_im_values(input_method_id, &[AttributeName::QueryInputStyle])
    }

    fn handle_get_im_values(
        &mut self,
        client: &mut C,
        input_method_id: u16,
        _attributes: AHashMap<AttributeName, Vec<u8>>,
    ) -> Result<(), ClientError> {
        let ic_attributes = client
            .build_ic_attributes()
            .push(AttributeName::InputStyle, InputStyle::PREEDIT_CALLBACKS)
            .push(AttributeName::ClientWindow, self.window)
            .push(AttributeName::FocusWindow, self.window)
            .build();
        self.client_window_bound = self.window != 0;
        client.create_ic(input_method_id, ic_attributes)
    }

    fn handle_create_ic(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        input_context_id: u16,
    ) -> Result<(), ClientError> {
        self.connected = true;
        self.ic_id = input_context_id;
        Ok(())
    }

    fn handle_commit(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
        text: &str,
    ) -> Result<(), ClientError> {
        self.apply_commit(text);
        Ok(())
    }

    fn handle_forward_event(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
        _flag: xim::ForwardEventFlag,
        xev: C::XEvent,
    ) -> Result<(), ClientError> {
        match xev.response_type {
            x11rb::protocol::xproto::KEY_PRESS_EVENT => {
                self.callback_events
                    .push_back(XimCallbackEvent::XimXEvent(Event::KeyPress(xev)));
            }
            x11rb::protocol::xproto::KEY_RELEASE_EVENT => {
                self.callback_events
                    .push_back(XimCallbackEvent::XimXEvent(Event::KeyRelease(xev)));
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_close(&mut self, client: &mut C, _input_method_id: u16) -> Result<(), ClientError> {
        client.disconnect()
    }

    fn handle_preedit_start(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
    ) -> Result<(), ClientError> {
        self.apply_preedit_start();
        Ok(())
    }

    fn handle_preedit_draw(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
        caret: i32,
        chg_first: i32,
        chg_len: i32,
        status: PreeditDrawStatus,
        preedit_string: &str,
        _feedbacks: Vec<xim::Feedback>,
    ) -> Result<(), ClientError> {
        self.apply_preedit_draw(caret, chg_first, chg_len, status, preedit_string);
        Ok(())
    }

    fn handle_preedit_caret(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
        position: &mut i32,
        direction: CaretDirection,
        _style: CaretStyle,
    ) -> Result<(), ClientError> {
        self.apply_preedit_caret(position, direction);
        Ok(())
    }

    fn handle_preedit_done(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
    ) -> Result<(), ClientError> {
        self.apply_preedit_done();
        Ok(())
    }

    fn handle_reset_ic(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
        _preedit_text: &str,
    ) -> Result<(), ClientError> {
        self.clear_preedit();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum ObservedEvent {
        Preedit(xproto::Window, String, Option<Range<usize>>),
        PreeditDone(xproto::Window),
        Commit(xproto::Window, String),
        XEvent,
    }

    fn drain_observed(handler: &mut XimHandler) -> Vec<ObservedEvent> {
        handler
            .callback_events
            .drain(..)
            .map(|event| match event {
                XimCallbackEvent::XimXEvent(_) => ObservedEvent::XEvent,
                XimCallbackEvent::XimPreeditEvent(win, text, cursor) => {
                    ObservedEvent::Preedit(win, text, cursor)
                }
                XimCallbackEvent::XimPreeditDoneEvent(win) => ObservedEvent::PreeditDone(win),
                XimCallbackEvent::XimCommitEvent(win, text) => ObservedEvent::Commit(win, text),
            })
            .collect()
    }

    #[test]
    fn incremental_preedit_draw_splice_caret_and_done() {
        let mut handler = XimHandler::new();
        handler.window = 42;

        handler.apply_preedit_start();
        // Insert "日本" at 0 with caret at char 2 (2 UTF-16 code units).
        handler.apply_preedit_draw(2, 0, 0, PreeditDrawStatus::empty(), "日本");
        // Append surrogate-pair emoji "🐢" at char 2 with caret at char 3 (4 UTF-16 code units).
        handler.apply_preedit_draw(3, 2, 0, PreeditDrawStatus::empty(), "🐢");
        // Delete "本" (char 1, len 1) using NO_STRING with caret at char 1.
        handler.apply_preedit_draw(1, 1, 1, PreeditDrawStatus::NO_STRING, "ignored");
        // Move caret forward over the surrogate pair "🐢" -> char 2 (3 UTF-16 code units).
        let mut pos = 0;
        handler.apply_preedit_caret(&mut pos, CaretDirection::ForwardChar);
        assert_eq!(pos, 2);

        // Preedit done clears buffer and emits done without losing earlier queued events.
        handler.apply_preedit_done();

        assert_eq!(
            drain_observed(&mut handler),
            vec![
                ObservedEvent::Preedit(42, "日本".into(), Some(2..2)),
                ObservedEvent::Preedit(42, "日本🐢".into(), Some(4..4)),
                ObservedEvent::Preedit(42, "日🐢".into(), Some(1..1)),
                ObservedEvent::Preedit(42, "日🐢".into(), Some(3..3)),
                ObservedEvent::PreeditDone(42),
            ]
        );
    }

    #[test]
    fn preedit_draw_clamps_out_of_bounds_and_commit_clears_buffer() {
        let mut handler = XimHandler::new();
        handler.window = 7;

        handler.apply_preedit_draw(99, -5, 99, PreeditDrawStatus::empty(), "かな");
        handler.apply_commit("仮名");
        // Next preedit draw starts fresh because commit cleared the preedit buffer.
        handler.apply_preedit_draw(1, 0, 0, PreeditDrawStatus::empty(), "あ");

        assert_eq!(
            drain_observed(&mut handler),
            vec![
                ObservedEvent::Preedit(7, "かな".into(), Some(2..2)),
                ObservedEvent::Commit(7, "仮名".into()),
                ObservedEvent::Preedit(7, "あ".into(), Some(1..1)),
            ]
        );
    }
}
