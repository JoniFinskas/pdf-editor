//! Opt-in interactive Windows acceptance test; controls only its own dialog.
use super::Picker;
use std::{ffi::c_void, time::{Duration, Instant}};
type Hwnd = *mut c_void;
#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(callback: unsafe extern "system" fn(Hwnd, isize) -> i32, data: isize) -> i32;
    fn GetWindowThreadProcessId(window: Hwnd, process: *mut u32) -> u32;
    fn GetClassNameW(window: Hwnd, name: *mut u16, max: i32) -> i32;
    fn IsWindowVisible(window: Hwnd) -> i32;
    fn IsWindowEnabled(window: Hwnd) -> i32;
    fn GetDlgItem(window: Hwnd, id: i32) -> Hwnd;
    fn PostMessageW(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> i32;
    fn SendMessageTimeoutW(window: Hwnd, message: u32, wparam: usize, lparam: isize, flags: u32, timeout: u32, result: *mut usize) -> isize;
}
unsafe extern "system" fn locate(window: Hwnd, out: isize) -> i32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid); }
    if pid != std::process::id() || unsafe { IsWindowVisible(window) } == 0 { return 1; }
    let mut class = [0u16; 64];
    let count = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    if count > 0 && String::from_utf16_lossy(&class[..count as usize]) == "#32770" {
        unsafe { *(out as *mut Hwnd) = window; }
        return 0;
    }
    1
}
fn dialog() -> Hwnd {
    let mut window = std::ptr::null_mut();
    unsafe { EnumWindows(locate, (&mut window as *mut Hwnd) as isize); }
    window
}
#[test]
#[ignore = "Opens and closes real Windows file dialogs; interactive desktop required"]
fn native_picker_open_cancel_timings() {
    let mut timings = Vec::new();
    for run in 0..5 {
        let mut picker = Picker::default();
        let started = Instant::now();
        assert!(picker.start(move || {
            let dialog = rfd::FileDialog::new().set_title("PDF Editor picker acceptance test")
                .add_filter("PDF document", &["pdf"]);
            dialog.pick_file()
        }).unwrap());
        let dispatch_ms = started.elapsed().as_millis();
        let deadline = started + Duration::from_secs(30);
        let mut frames = 0;
        let window = loop {
            let window = dialog();
            if !window.is_null() {
                let mut reply = 0;
                if unsafe { SendMessageTimeoutW(window, 0, 0, 0, 2, 100, &mut reply) } != 0 { break window; }
            }
            assert!(Instant::now() < deadline, "Native dialog did not become responsive");
            assert!(picker.poll().is_none(), "Picker returned before showing its dialog");
            frames += 1;
            std::thread::sleep(Duration::from_millis(5));
        };
        let ready_ms = started.elapsed().as_millis();
        let closing = Instant::now();
        eprintln!("native_picker ready: run={run} dispatch_ms={dispatch_ms} ready_ms={ready_ms}");
        let button = unsafe { GetDlgItem(window, 2) };
        assert!(!button.is_null(), "Dialog action button is unavailable");
        while unsafe { IsWindowEnabled(button) } == 0 {
            assert!(Instant::now() < deadline, "Dialog action did not become enabled");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_ne!(unsafe { PostMessageW(button, 0x00F5, 0, 0) }, 0);
        let result = loop {
            if let Some(result) = picker.poll() { break result.unwrap(); }
            assert!(Instant::now() < deadline, "Native dialog did not close");
            frames += 1;
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(result.is_none());
        assert!(!picker.pending());
        let sample = (run, dispatch_ms, ready_ms, closing.elapsed().as_millis(), frames);
        eprintln!("native_picker completed: {sample:?}");
        timings.push(sample);
    }
    eprintln!("native_picker (run, dispatch_ms, ready_ms, completion_ms, responsive_poll_ticks): {timings:?}");
}

#[test]
#[ignore = "Selects generated paths in real Windows Open and Save As dialogs"]
fn native_open_and_save_as_select_generated_paths() {
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let folder=root.join(format!("target/native-picker-files-{}",std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let input=folder.join("generated input.pdf");let output=folder.join("generated output.pdf");
    std::fs::copy(root.join("tests/fixtures/generated/unicode-cid.pdf"),&input).unwrap();
    let original=std::fs::read(&input).unwrap();
    for save in [false,true] {
        let expected=if save {output.clone()} else {input.clone()};
        let mut picker=Picker::default();let parent=folder.canonicalize().unwrap();let name=expected.file_name().unwrap().to_string_lossy().into_owned();
        picker.start(move || {
            let dialog=rfd::FileDialog::new().set_title("PDF Editor generated picker acceptance").set_directory(parent).set_file_name(name).add_filter("PDF",&["pdf"]);
            if save {dialog.save_file()}else{dialog.pick_file()}
        }).unwrap();
        eprintln!("native mode save={save}");
        crate::native_controls::select_file(&expected).unwrap();
        let deadline=Instant::now()+Duration::from_secs(25);
        let selected=loop {if let Some(result)=picker.poll(){break result.unwrap().unwrap();}
            assert!(Instant::now()<deadline,"Native picker did not complete");std::thread::sleep(Duration::from_millis(10));};
        assert_eq!(selected,expected);
    }
    assert_eq!(std::fs::read(&input).unwrap(),original);assert!(!output.exists());
    std::fs::remove_dir_all(folder).unwrap();
}
