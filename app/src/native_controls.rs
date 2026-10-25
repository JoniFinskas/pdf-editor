//! Test-only Win32 controls, restricted to windows owned by this process.
use std::{
    ffi::c_void,
    path::Path,
    time::{Duration, Instant},
};
type Hwnd = *mut c_void;
#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(callback: unsafe extern "system" fn(Hwnd, isize) -> i32, data: isize) -> i32;
    fn EnumChildWindows(
        window: Hwnd,
        callback: unsafe extern "system" fn(Hwnd, isize) -> i32,
        data: isize,
    ) -> i32;
    fn GetWindowThreadProcessId(window: Hwnd, pid: *mut u32) -> u32;
    fn GetClassNameW(window: Hwnd, text: *mut u16, length: i32) -> i32;
    fn GetWindowTextW(window: Hwnd, text: *mut u16, length: i32) -> i32;
    fn IsWindowVisible(window: Hwnd) -> i32;
    fn IsWindowEnabled(window: Hwnd) -> i32;
    fn SetForegroundWindow(window: Hwnd) -> i32;
    fn GetForegroundWindow() -> Hwnd;
    fn SetFocus(window: Hwnd) -> Hwnd;
    fn AttachThreadInput(from: u32, to: u32, attach: i32) -> i32;
    fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
    fn GetPropW(window: Hwnd, name: *const u16) -> *mut c_void;
    fn GetDlgCtrlID(window: Hwnd) -> i32;
    fn GetParent(window: Hwnd) -> Hwnd;
    fn GetDlgItem(window: Hwnd, id: i32) -> Hwnd;
    fn PostMessageW(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> i32;
    fn SendMessageTimeoutW(
        window: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        flags: u32,
        timeout: u32,
        result: *mut usize,
    ) -> isize;
}

#[repr(C)]
struct Guid {
    a: u32,
    b: u16,
    c: u16,
    d: [u8; 8],
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn SHParseDisplayName(
        name: *const u16,
        bind: *mut c_void,
        pidl: *mut *mut c_void,
        mask: u32,
        attributes: *mut u32,
    ) -> i32;
    fn ILClone(pidl: *const c_void) -> *mut c_void;
    fn ILRemoveLastID(pidl: *mut c_void) -> i32;
    fn ILFindLastID(pidl: *const c_void) -> *mut c_void;
    fn DragQueryFileW(drop: *mut c_void, index: u32, name: *mut u16, length: u32) -> u32;
    fn SHCreateDataObject(
        folder: *const c_void,
        count: u32,
        children: *const *mut c_void,
        inner: *mut c_void,
        iid: *const Guid,
        out: *mut *mut c_void,
    ) -> i32;
}
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(memory: *mut c_void);
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
    fn ReleaseStgMedium(medium: *mut Medium);
}
#[repr(C)]
struct Format {
    format: u16,
    target: *mut c_void,
    aspect: u32,
    index: i32,
    medium: u32,
}
#[repr(C)]
struct Medium {
    kind: u32,
    data: *mut c_void,
    release: *mut c_void,
}
#[repr(C)]
struct DataMethods {
    unknown: [usize; 3],
    get: unsafe extern "system" fn(*mut c_void, *const Format, *mut Medium) -> i32,
}
#[repr(C)]
struct DropMethods {
    unknown: [usize; 3],
    enter: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *const c_void, *mut u32) -> i32,
    over: usize,
    leave: unsafe extern "system" fn(*mut c_void) -> i32,
    drop: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *const c_void, *mut u32) -> i32,
}
#[repr(C)]
struct UnknownMethods {
    query: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// Deliver the same shell data object that Explorer supplies to the registered
/// OLE target, without moving the desktop cursor or accessing Explorer.
pub(crate) fn drop_generated_file(title: &str, path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let window = find(Some(title), false);
    if window.is_null() || !owned(window) {
        return Err("Owned drop window unavailable".into());
    }
    let property = "OleDropTargetInterface"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target = unsafe { GetPropW(window, property.as_ptr()) };
    if target.is_null() {
        return Err("Owned native OLE drop target unavailable".into());
    }
    let normalized = path
        .canonicalize()
        .map_err(|_| "Generated drop file unavailable")?;
    let normalized = normalized.to_string_lossy().replace('/', "\\");
    let normalized = normalized.strip_prefix(r"\\?\").unwrap_or(&normalized);
    let name = Path::new(normalized)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut pidl = std::ptr::null_mut();
    let mut attributes = 0;
    let initialized = unsafe { CoInitializeEx(std::ptr::null_mut(), 2) };
    struct Apartment(bool);
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }
    let _apartment = Apartment(initialized >= 0);
    let parsed = unsafe {
        SHParseDisplayName(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut pidl,
            0,
            &mut attributes,
        )
    };
    if parsed < 0 {
        return Err(format!("Cannot create generated shell item: {parsed:#x}"));
    }
    let iid = Guid {
        a: 0x10e,
        b: 0,
        c: 0,
        d: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
    };
    let mut data = std::ptr::null_mut();
    let parent = unsafe { ILClone(pidl) };
    if parent.is_null() {
        unsafe { CoTaskMemFree(pidl) };
        return Err("Cannot clone generated shell parent".into());
    }
    unsafe { ILRemoveLastID(parent) };
    let child = unsafe { ILFindLastID(pidl) };
    let created =
        unsafe { SHCreateDataObject(parent, 1, &child, std::ptr::null_mut(), &iid, &mut data) };
    unsafe {
        CoTaskMemFree(parent);
        CoTaskMemFree(pidl)
    };
    if created < 0 || data.is_null() {
        return Err("Cannot create generated shell drop data".into());
    }
    let data_methods = unsafe { &**(data as *const *const DataMethods) };
    let format = Format {
        format: 15,
        target: std::ptr::null_mut(),
        aspect: 1,
        index: -1,
        medium: 1,
    };
    let mut medium = Medium {
        kind: 0,
        data: std::ptr::null_mut(),
        release: std::ptr::null_mut(),
    };
    let result = unsafe { (data_methods.get)(data, &format, &mut medium) };
    let count = if result >= 0 {
        let count = unsafe { DragQueryFileW(medium.data, u32::MAX, std::ptr::null_mut(), 0) };
        unsafe { ReleaseStgMedium(&mut medium) };
        count
    } else {
        0
    };
    eprintln!("native shell drop: result={result:#x}, files={count}");
    let methods = unsafe { &**(target as *const *const DropMethods) };
    let mut effect = 1;
    let entered = unsafe { (methods.enter)(target, data, 1, std::ptr::null(), &mut effect) };
    let dropped = if entered >= 0 {
        unsafe { (methods.drop)(target, data, 0, std::ptr::null(), &mut effect) }
    } else {
        unsafe { (methods.leave)(target) };
        entered
    };
    let release = unsafe { &**(data as *const *const UnknownMethods) };
    unsafe { (release.release)(data) };
    if dropped < 0 {
        return Err("Native generated drop failed".into());
    }
    Ok(())
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
}
#[repr(C)]
#[derive(Clone, Copy)]
struct KeyInput {
    key: u16,
    scan: u16,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
union InputData {
    keyboard: KeyInput,
    padding: [u64; 4],
}
#[repr(C)]
struct Input {
    kind: u32,
    data: InputData,
}
fn owned(window: Hwnd) -> bool {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    pid == std::process::id()
}
fn text(window: Hwnd, class: bool) -> String {
    let mut value = [0u16; 256];
    let count = unsafe {
        if class {
            GetClassNameW(window, value.as_mut_ptr(), 256)
        } else {
            GetWindowTextW(window, value.as_mut_ptr(), 256)
        }
    };
    String::from_utf16_lossy(&value[..count.max(0) as usize])
}
struct Find {
    title: Option<String>,
    dialog: bool,
    window: Hwnd,
}
unsafe extern "system" fn locate(window: Hwnd, data: isize) -> i32 {
    let find = unsafe { &mut *(data as *mut Find) };
    if owned(window)
        && unsafe { IsWindowVisible(window) } != 0
        && (text(window, true) == "#32770") == find.dialog
        && find
            .title
            .as_ref()
            .is_none_or(|title| text(window, false) == *title)
    {
        find.window = window;
        return 0;
    }
    1
}
fn find(title: Option<&str>, dialog: bool) -> Hwnd {
    let mut query = Find {
        title: title.map(str::to_string),
        dialog,
        window: std::ptr::null_mut(),
    };
    unsafe { EnumWindows(locate, (&mut query as *mut Find) as isize) };
    query.window
}
unsafe extern "system" fn filename(window: Hwnd, data: isize) -> i32 {
    if !owned(window) || unsafe { IsWindowVisible(window) } == 0 || text(window, true) != "Edit" {
        return 1;
    }
    let id = unsafe { GetDlgCtrlID(window) };
    let parent = unsafe { GetParent(window) };
    if id == 1148
        || unsafe { GetDlgCtrlID(parent) } == 1148
        || matches!(text(parent, true).as_str(), "ComboBox" | "ComboBoxEx32")
    {
        unsafe { *(data as *mut Hwnd) = window };
        return 0;
    }
    1
}
pub(crate) fn select_file(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let parent = path
        .parent()
        .ok_or("Generated native path has no parent")?
        .canonicalize()
        .map_err(|_| "Generated native folder unavailable")?;
    let normalized = parent
        .join(
            path.file_name()
                .ok_or("Generated native filename unavailable")?,
        )
        .to_string_lossy()
        .replace('/', "\\");
    let path = Path::new(normalized.strip_prefix(r"\\?\").unwrap_or(&normalized));

    let deadline = Instant::now() + Duration::from_secs(25);
    let window = loop {
        let window = find(None, true);
        if !window.is_null() {
            break window;
        }
        if Instant::now() > deadline {
            return Err("Owned native dialog did not appear".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut field: Hwnd = std::ptr::null_mut();
    while field.is_null() {
        unsafe { EnumChildWindows(window, filename, (&mut field as *mut Hwnd) as isize) };
        if Instant::now() > deadline {
            return Err("Native filename field unavailable".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let mut result = 0;
    let empty = [0u16];
    if !owned(field)
        || unsafe {
            SendMessageTimeoutW(
                field,
                0x000c,
                0,
                empty.as_ptr() as isize,
                2,
                1000,
                &mut result,
            )
        } == 0
    {
        return Err("Cannot clear owned native filename field".into());
    }
    unsafe {
        SendMessageTimeoutW(window, 0x0028, field as usize, 1, 2, 1000, &mut result);
    }
    let foreground_input = std::env::var_os("PDF_EDITOR_NATIVE_KEYBOARD").is_some();
    let target = unsafe { GetWindowThreadProcessId(field, std::ptr::null_mut()) };
    let caller = unsafe { GetCurrentThreadId() };
    unsafe {
        if foreground_input {
            SetForegroundWindow(window);
        }
        AttachThreadInput(caller, target, 1);
        SetFocus(field);
        AttachThreadInput(caller, target, 0);
    }
    let send = |key: u16, scan: u16, flags: u32| -> Result<(), String> {
        if unsafe { GetForegroundWindow() } != window || !owned(window) {
            return Err("Native test lost foreground ownership".into());
        }
        let inputs = [
            Input {
                kind: 1,
                data: InputData {
                    keyboard: KeyInput {
                        key,
                        scan,
                        flags,
                        time: 0,
                        extra: 0,
                    },
                },
            },
            Input {
                kind: 1,
                data: InputData {
                    keyboard: KeyInput {
                        key,
                        scan,
                        flags: flags | 2,
                        time: 0,
                        extra: 0,
                    },
                },
            },
        ];
        if unsafe { SendInput(2, inputs.as_ptr(), std::mem::size_of::<Input>() as i32) } != 2 {
            return Err("Native test input unavailable".into());
        }
        Ok(())
    };
    for character in wide {
        if foreground_input {
            send(0, character, 4)?;
        } else if unsafe { PostMessageW(field, 0x0102, character as usize, 1) } == 0 {
            return Err("Cannot type owned native filename".into());
        }
    }
    while text(field, false) != path.to_string_lossy() {
        if Instant::now() > deadline {
            return Err("Native filename did not update".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(300));
    eprintln!(
        "native filename accepted={}, field_id={}",
        text(field, false) == path.to_string_lossy(),
        unsafe { GetDlgCtrlID(field) }
    );
    let button = unsafe { GetDlgItem(window, 1) };
    eprintln!("native accept button: {}", text(button, false));
    if button.is_null() || !owned(button) {
        return Err("Owned native accept button unavailable".into());
    }
    while unsafe { IsWindowEnabled(button) } == 0 {
        if Instant::now() > deadline {
            return Err("Native accept button did not enable".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if foreground_input {
        send(0x0d, 0, 0)?;
    } else {
        unsafe {
            PostMessageW(field, 0x0100, 0x0d, 1);
            PostMessageW(field, 0x0101, 0x0d, 0xc0000001_u32 as isize);
        }
    }
    Ok(())
}
pub(crate) fn close_window(title: &str) -> Result<(), String> {
    let window = find(Some(title), false);
    if window.is_null() || !owned(window) || unsafe { PostMessageW(window, 0x0010, 0, 0) } == 0 {
        return Err("Cannot close owned acceptance window".into());
    }
    Ok(())
}
