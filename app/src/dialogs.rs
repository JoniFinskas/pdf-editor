//! Native pickers run outside the frame loop; cancellation is a completed result.
use std::{path::PathBuf, sync::mpsc, thread};

pub struct Picker<T = PathBuf> {
    result: Option<mpsc::Receiver<Result<Option<T>, &'static str>>>,
}
impl<T> Default for Picker<T> {
    fn default() -> Self {
        Self { result: None }
    }
}
impl<T: Send + 'static> Picker<T> {
    pub fn pending(&self) -> bool {
        self.result.is_some()
    }

    pub fn start(
        &mut self,
        task: impl FnOnce() -> Option<T> + Send + 'static,
    ) -> std::io::Result<bool> {
        if self.pending() {
            return Ok(false);
        }
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("file-picker".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task))
                    .map_err(|_| "The file picker could not be completed. Try again.");
                let _ = sender.send(result);
            })?;
        self.result = Some(receiver);
        Ok(true)
    }

    pub fn poll(&mut self) -> Option<Result<Option<T>, &'static str>> {
        let receiver = self.result.as_ref()?;
        match receiver.try_recv() {
            Ok(result) => {
                self.result = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.result = None;
                Some(Err("The file picker closed unexpectedly. Try again."))
            }
        }
    }
}

/// A tool-owned pending action. Inputs are captured at launch, never borrowed
/// from a later frame or another tab. Closing/cancelling discards late results.
pub struct ToolDialog<T> {
    picker: Picker<T>,
    ready: Option<T>,
    discarded: bool,
    error: Option<String>,
}
impl<T> Default for ToolDialog<T> {
    fn default() -> Self {
        Self {
            picker: Picker::default(),
            ready: None,
            discarded: false,
            error: None,
        }
    }
}
impl<T: Send + 'static> ToolDialog<T> {
    pub fn busy(&self) -> bool {
        self.picker.pending() || self.ready.is_some()
    }
    pub fn launch(&mut self, task: impl FnOnce() -> Option<T> + Send + 'static) {
        if self.busy() {
            return;
        }
        self.discarded = false;
        self.error = None;
        if let Err(error) = self.picker.start(task) {
            self.error = Some(format!("Cannot start file selection: {error}"));
        }
    }
    pub fn poll(&mut self, ctx: &eframe::egui::Context, open: bool, idle: bool) -> Option<T> {
        if !open {
            self.cancel();
        }
        if self.picker.pending() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if let Some(result) = self.picker.poll() {
            match result {
                Ok(value) if !self.discarded => self.ready = value,
                Err(error) if !self.discarded => self.error = Some(error.into()),
                _ => {}
            }
        }
        if open && idle {
            self.ready.take()
        } else {
            None
        }
    }
    pub fn cancel(&mut self) {
        self.discarded = true;
        self.ready = None;
    }
    pub fn feedback(&mut self, ui: &mut eframe::egui::Ui, p: &crate::theme::Palette) {
        self.feedback_for(ui,p,"Waiting for file selection…",true);
    }
    pub fn feedback_for(&mut self, ui: &mut eframe::egui::Ui, p: &crate::theme::Palette, activity: &str, cancellable: bool) {
        if self.busy() {
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label(if self.discarded {
                    if cancellable { "Action cancelled. Close the file dialog to continue." } else { "Finishing draft operation…" }
                } else if self.ready.is_some() {
                    "Waiting for the current operation…"
                } else {
                    activity
                });
                if !self.discarded
                    && cancellable
                    && crate::widgets::Btn::new("Cancel pending action")
                        .show(ui, p)
                        .clicked()
                {
                    self.cancel();
                }
            });
        }
        if let Some(error) = self.error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(p.error_fg, error);
                if crate::widgets::Btn::new("Dismiss").show(ui, p).clicked() {
                    self.error = None;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_results_are_owned_deferred_once_and_discarded_after_close() {
        let ctx = eframe::egui::Context::default();
        let mut first = ToolDialog::default();
        let mut other = ToolDialog::<Vec<String>>::default();
        let (release, wait) = mpsc::channel();
        first.launch(move || {
            wait.recv().unwrap();
            Some(vec!["generated.png".to_string()])
        });
        first.launch(|| panic!("Duplicate job must not start"));
        assert!(first.poll(&ctx, true, true).is_none());
        assert!(other.poll(&ctx, true, true).is_none());
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while first.ready.is_none() {
            assert!(first.poll(&ctx, true, false).is_none());
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(first.poll(&ctx, true, true).unwrap(), vec!["generated.png"]);
        assert!(first.poll(&ctx, true, true).is_none());
        for explicit in [false, true] {
            let (release, wait) = mpsc::channel();
            first.launch(move || {
                wait.recv().unwrap();
                Some(vec!["discarded".to_string()])
            });
            if explicit {
                first.cancel();
            } else {
                first.poll(&ctx, false, true);
            }
            release.send(()).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while first.busy() {
                assert!(first.poll(&ctx, true, true).is_none());
                assert!(std::time::Instant::now() < deadline);
                thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }
    #[test]
    fn tool_panics_become_a_dismissible_error_and_allow_retry() {
        let ctx = eframe::egui::Context::default();
        let mut task = ToolDialog::<usize>::default();
        task.launch(|| panic!("simulated picker failure"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while task.busy() {
            assert!(task.poll(&ctx, true, true).is_none());
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(task.error.is_some());
        task.launch(|| None);
        assert!(task.error.is_none());
    }
    #[test]
    fn picker_does_not_block_and_repeated_cancel_releases_it() {
        let mut picker = Picker::default();
        for selected in [None, Some(PathBuf::from("generated.pdf")), None] {
            let (release, wait) = mpsc::channel();
            let expected = selected.clone();
            assert!(
                picker
                    .start(move || {
                        wait.recv().unwrap();
                        selected
                    })
                    .unwrap()
            );
            assert!(picker.pending());
            assert!(picker.poll().is_none());
            assert!(
                !picker
                    .start(|| panic!("Duplicate picker must not run"))
                    .unwrap()
            );
            release.send(()).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let result = loop {
                if let Some(result) = picker.poll() {
                    break result;
                }
                assert!(std::time::Instant::now() < deadline);
                thread::sleep(std::time::Duration::from_millis(1));
            };
            assert_eq!(result.unwrap(), expected);
            assert!(!picker.pending());
        }
    }
}

#[cfg(all(test, windows))]
#[path = "dialog_native_tests.rs"]
mod native_tests;
