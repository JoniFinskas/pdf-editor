/// Tracks the saved position of a linear undo history, including abandoned redo branches.
#[derive(Clone, Debug)]
pub struct ChangeState {
    position: i64,
    saved: Option<i64>,
}
impl Default for ChangeState {
    fn default() -> Self {
        Self {
            position: 0,
            saved: Some(0),
        }
    }
}
impl ChangeState {
    pub fn recovered() -> Self {
        Self {
            position: 0,
            saved: None,
        }
    }
    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.position)
    }
    pub fn edited(&mut self) {
        if self.saved.is_some_and(|saved| saved > self.position) {
            self.saved = None;
        }
        self.position += 1;
    }
    pub fn undone(&mut self) {
        self.position -= 1;
    }
    pub fn redone(&mut self) {
        self.position += 1;
    }
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_points_follow_undo_redo_and_abandoned_branches() {
        let mut state = ChangeState::default();
        assert!(!state.is_dirty());
        state.edited();
        assert!(state.is_dirty());
        state.mark_saved();
        assert!(!state.is_dirty());
        state.undone();
        assert!(state.is_dirty());
        state.redone();
        assert!(!state.is_dirty());
        state.undone();
        state.edited();
        assert!(state.is_dirty());
        state.undone();
        state.redone();
        assert!(state.is_dirty());
        let mut recovered = ChangeState::recovered();
        assert!(recovered.is_dirty());
        recovered.mark_saved();
        assert!(!recovered.is_dirty());
    }
}
