//! The `zwp_text_input_v3` state machine.
//!
//! The protocol is double-buffered: `preedit_string`, `commit_string` and
//! `delete_surrounding_text` only stage state, and a `done` applies the whole
//! batch. Slint draws the composition itself once the batch is handed to it as
//! an internal key event.

/// One staged batch of text-input events.
#[derive(Debug, Default)]
pub struct Pending {
    preedit: Option<(String, Option<(i32, i32)>)>,
    commit: Option<String>,
    delete: Option<(u32, u32)>,
}

/// What a `done` resolves the staged batch to.
#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    /// Nothing staged and no composition was open.
    Idle,
    /// A new (possibly empty) composition to draw.
    Preedit {
        text: String,
        selection: Option<(i32, i32)>,
    },
    /// The finished text, with the surrounding range it replaces.
    Commit {
        text: String,
        delete: Option<(u32, u32)>,
    },
}

impl Pending {
    /// `cursor_begin`/`cursor_end` are byte offsets within the preedit, or -1.
    pub fn preedit(&mut self, text: String, cursor_begin: i32, cursor_end: i32) {
        let selection =
            (cursor_begin >= 0 && cursor_end >= 0).then_some((cursor_begin, cursor_end));
        self.preedit = Some((text, selection));
    }

    pub fn commit(&mut self, text: String) {
        self.commit = Some(text);
    }

    pub fn delete(&mut self, before: u32, after: u32) {
        self.delete = Some((before, after));
    }

    /// Applies the staged batch. A `done` that staged nothing ends the
    /// composition: fcitx5+rime finishes by backspacing its last preedit
    /// character and sending a bare `done`, so a client that only clears on an
    /// explicit empty `preedit_string` leaves the field drawing a composition
    /// the input method has already closed.
    pub fn resolve(&mut self, composing: bool) -> Applied {
        let batch = std::mem::take(self);
        if let Some(text) = batch.commit {
            return Applied::Commit {
                text,
                delete: batch.delete,
            };
        }
        if let Some((text, selection)) = batch.preedit {
            return Applied::Preedit { text, selection };
        }
        if composing {
            Applied::Preedit {
                text: String::new(),
                selection: None,
            }
        } else {
            Applied::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_batch_only_clears_an_open_composition() {
        let mut pending = Pending::default();
        assert_eq!(pending.resolve(false), Applied::Idle);
        assert_eq!(
            pending.resolve(true),
            Applied::Preedit {
                text: String::new(),
                selection: None
            },
            "a bare done ends the composition the field is still drawing"
        );
    }

    #[test]
    fn a_staged_preedit_is_applied_with_its_selection() {
        let mut pending = Pending::default();
        pending.preedit("にほん".into(), 0, 6);
        assert_eq!(
            pending.resolve(true),
            Applied::Preedit {
                text: "にほん".into(),
                selection: Some((0, 6))
            }
        );
        // The batch is consumed: the next bare done clears instead of repeating.
        assert_eq!(
            pending.resolve(true),
            Applied::Preedit {
                text: String::new(),
                selection: None
            }
        );
    }

    #[test]
    fn a_commit_wins_over_the_preedit_it_replaces() {
        let mut pending = Pending::default();
        pending.preedit("nihon".into(), 5, 5);
        pending.commit("日本".into());
        assert_eq!(
            pending.resolve(true),
            Applied::Commit {
                text: "日本".into(),
                delete: None
            }
        );
    }

    #[test]
    fn a_commit_carries_the_surrounding_range_it_deletes() {
        let mut pending = Pending::default();
        pending.delete(3, 1);
        pending.commit("x".into());
        assert_eq!(
            pending.resolve(true),
            Applied::Commit {
                text: "x".into(),
                delete: Some((3, 1))
            }
        );
    }
}
