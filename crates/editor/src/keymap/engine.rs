// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The modal keymap engine: a stack of modes and a pending sequence,
//! resolving keys as they go down (the [`keymap`](super) module
//! documentation, "The engine").
//!
//! New in Tidepool.

use tidepool_core::input::InputEvent;

use super::{Action, Keymap, Lookup, MAX_MODE_DEPTH, Step};

/// What a key going down came to ([`Engine::handle`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution<'a> {
    /// Not a key going down, or a modifier key alone: nothing changed, and
    /// the key is not the keymap's.
    Ignored,
    /// No binding of the top mode starts with the key, and no sequence was
    /// pending: the key is not the keymap's, and its owner may use it.
    Unbound,
    /// The key starts or continues a binding: the engine waits for the
    /// next key, with no timeout.
    Pending,
    /// The key continues no binding of the pending steps: they are
    /// dropped, and the key is consumed.
    Canceled,
    /// The key finished a binding of a registered command, which the
    /// engine's owner runs.
    Command {
        /// The command's id.
        id: &'a str,
        /// Whether the key going down was an auto-repeat.
        repeat: bool,
    },
    /// The key finished a binding of one of the engine's commands,
    /// `mode.enter.<mode>` or `mode.leave`, which it ran.
    Mode {
        /// Whether the mode changed: it does not when `mode.leave` would
        /// leave the last mode, or `mode.enter` would take the stack past
        /// [`MAX_MODE_DEPTH`].
        changed: bool,
        /// Whether the key going down was an auto-repeat.
        repeat: bool,
    },
}

/// The modal keymap engine over a [`Keymap`]: its stack of modes and the
/// steps of a sequence it waits to finish (the [`keymap`](super) module
/// documentation, "The engine").
#[derive(Clone, Debug)]
pub struct Engine {
    keymap: Keymap,
    /// The modes entered, as indexes of the keymap's modes: never empty,
    /// the start mode at the bottom, at most [`MAX_MODE_DEPTH`].
    stack: Vec<usize>,
    /// The steps of the sequence pending: fewer than the longest binding's.
    pending: Vec<Step>,
}

impl Engine {
    /// An engine over `keymap`, in its start mode, with nothing pending.
    pub fn new(keymap: Keymap) -> Self {
        let start = keymap.start;
        Self {
            keymap,
            stack: vec![start],
            pending: Vec::new(),
        }
    }

    /// Its keymap.
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// The mode on top of the stack, whose bindings keys are looked up in.
    pub fn mode(&self) -> &str {
        self.keymap.mode_name(self.top())
    }

    /// The stack of modes, from the start mode at the bottom to the top.
    pub fn stack(&self) -> impl Iterator<Item = &str> {
        self.stack.iter().map(|&mode| self.keymap.mode_name(mode))
    }

    /// The steps of the sequence it waits to finish: empty when none is
    /// pending.
    pub fn pending(&self) -> &[Step] {
        &self.pending
    }

    /// Resolves `event`, if it is a key going down, as the [module
    /// documentation](super#the-engine) says.
    pub fn handle(&mut self, event: &InputEvent) -> Resolution<'_> {
        let InputEvent::KeyDown {
            key,
            modifiers,
            repeat,
        } = *event
        else {
            return Resolution::Ignored;
        };
        match Step::new(modifiers, key) {
            Some(step) => self.press(step, repeat),
            None => Resolution::Ignored,
        }
    }

    /// Resolves `step` going down, an auto-repeat if `repeat`: what
    /// [`handle`](Self::handle) does with a key going down that is not a
    /// modifier key.
    pub fn press(&mut self, step: Step, repeat: bool) -> Resolution<'_> {
        let top = self.top();
        self.pending.push(step);
        let found = match self.keymap.lookup(top, &self.pending) {
            Lookup::Found(at) => at,
            Lookup::Prefix => return Resolution::Pending,
            Lookup::Nothing => {
                let was_pending = self.pending.len() > 1;
                self.pending.clear();
                return if was_pending {
                    Resolution::Canceled
                } else {
                    Resolution::Unbound
                };
            }
        };
        self.pending.clear();
        let Self { keymap, stack, .. } = self;
        let Some(binding) = keymap
            .modes
            .get(top)
            .and_then(|mode| mode.bindings.get(found))
        else {
            return Resolution::Unbound;
        };
        match binding.action {
            Action::Command => Resolution::Command {
                id: &binding.command,
                repeat,
            },
            Action::Enter(mode) => Resolution::Mode {
                changed: push(stack, mode),
                repeat,
            },
            Action::Leave => Resolution::Mode {
                changed: pop(stack),
                repeat,
            },
        }
    }

    /// Enters `mode`, on top of the stack, and drops the pending steps.
    /// Returns whether it did: not for a mode the keymap does not have, nor
    /// past [`MAX_MODE_DEPTH`].
    pub fn enter(&mut self, mode: &str) -> bool {
        let Some(mode) = self.keymap.mode_index(mode) else {
            return false;
        };
        let changed = push(&mut self.stack, mode);
        if changed {
            self.pending.clear();
        }
        changed
    }

    /// Leaves the mode on top of the stack, unless it is the last, and
    /// drops the pending steps. Returns whether it did.
    pub fn leave(&mut self) -> bool {
        let changed = pop(&mut self.stack);
        if changed {
            self.pending.clear();
        }
        changed
    }

    /// Drops the pending steps, if any.
    pub fn cancel(&mut self) {
        self.pending.clear();
    }

    /// The index of the mode on top of the stack.
    fn top(&self) -> usize {
        self.stack.last().copied().unwrap_or(self.keymap.start)
    }
}

/// Pushes `mode` onto `stack`, unless that would take it past
/// [`MAX_MODE_DEPTH`]. Returns whether it did.
fn push(stack: &mut Vec<usize>, mode: usize) -> bool {
    if stack.len() >= MAX_MODE_DEPTH {
        return false;
    }
    stack.push(mode);
    true
}

/// Pops the top of `stack`, unless it is the last. Returns whether it did.
fn pop(stack: &mut Vec<usize>) -> bool {
    if stack.len() <= 1 {
        return false;
    }
    stack.pop();
    true
}
