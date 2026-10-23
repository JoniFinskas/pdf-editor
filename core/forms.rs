use crate::*;

#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Text(String),
    Check(bool),
    Radio(bool),
    Choice {
        options: Vec<String>,
        selected: Option<usize>,
    },
}
#[derive(Clone, Debug)]
pub struct FormField {
    pub page: u16,
    pub index: u16,
    pub revision: u64,
    pub name: String,
    pub read_only: bool,
    pub value: FieldValue,
}
impl Engine {
    pub fn form_fields(&self, doc: &Document) -> Result<Vec<FormField>> {
        guarded(|| {
            let native = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let mut out = vec![];
            for (page_no, page) in native.pages().iter().enumerate() {
                for (index, annotation) in page.annotations().iter().enumerate() {
                    let Some(field) = annotation.as_form_field() else {
                        continue;
                    };
                    let value = match field {
                        PdfFormField::Text(f) => FieldValue::Text(f.value().unwrap_or_default()),
                        PdfFormField::Checkbox(f) => FieldValue::Check(
                            f.appearance_stream()
                                .map_or(f.is_checked()?, |s| s != "Off" && s != "/Off"),
                        ),
                        PdfFormField::RadioButton(f) => FieldValue::Radio(
                            f.appearance_stream()
                                .is_some_and(|s| s != "Off" && s != "/Off"),
                        ),
                        PdfFormField::ComboBox(f) => {
                            let options: Vec<_> = f.options().iter().collect();
                            FieldValue::Choice {
                                selected: options.iter().position(|o| o.is_set()),
                                options: options
                                    .iter()
                                    .map(|o| o.label().cloned().unwrap_or_default())
                                    .collect(),
                            }
                        }
                        _ => continue,
                    };
                    if out.len() >= 10000 {
                        return Err(err("Too many form fields"));
                    }
                    out.push(FormField {
                        page: page_no as u16,
                        index: index as u16,
                        revision: doc.revision,
                        name: field
                            .name()
                            .unwrap_or_else(|| format!("Field {}", index + 1)),
                        read_only: field.is_read_only(),
                        value,
                    });
                }
            }
            Ok(out)
        })
    }
    pub fn fill_field(
        &self,
        doc: &mut Document,
        selected: &FormField,
        value: &FieldValue,
    ) -> Result<()> {
        guarded(|| {
            if selected.revision != doc.revision {
                return Err(err("Form changed; refresh the fields"));
            }
            let fields = self.form_fields(doc)?;
            let actual = fields
                .iter()
                .find(|f| f.page == selected.page && f.index == selected.index)
                .ok_or_else(|| err("Field no longer exists"))?;
            if actual.read_only {
                return Err(err("This field is read-only"));
            }
            if std::mem::discriminant(&actual.value) != std::mem::discriminant(value) {
                return Err(err("Field type mismatch"));
            }
            if let FieldValue::Text(text) = value
                && (text.len() > 16384 || text.contains('\0'))
            {
                return Err(err("Form text exceeds the supported length"));
            }
            let bindings = self.raw;
            // The wrapper exposes field values but not interactive appearance regeneration.
            // Keep the backing bytes, pinned callback table and native handles alive together.
            let mut native = NativeForm::open(bindings, &doc.bytes, selected.page)?;
            unsafe {
                let annotation = bindings.FPDFPage_GetAnnot(native.page, i32::from(selected.index));
                if annotation.is_null() {
                    return Err(err("Field no longer exists"));
                }
                let mut rect = FS_RECTF {
                    left: 0.0,
                    bottom: 0.0,
                    right: 0.0,
                    top: 0.0,
                };
                let valid = bindings.FPDFAnnot_GetRect(annotation, &mut rect) != 0;
                bindings.FPDFPage_CloseAnnot(annotation);
                if !valid {
                    return Err(err("Invalid field bounds"));
                }
                let x = f64::from((rect.left + rect.right) / 2.0);
                let y = f64::from((rect.bottom + rect.top) / 2.0);
                bindings.FORM_OnFocus(native.form, native.page, 0, x, y);
                match value {
                    FieldValue::Text(text) => {
                        bindings.FORM_SelectAllText(native.form, native.page);
                        let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
                        bindings.FORM_ReplaceSelection(native.form, native.page, text.as_ptr());
                    }
                    FieldValue::Check(checked) | FieldValue::Radio(checked) => {
                        let previous = matches!(
                            actual.value,
                            FieldValue::Check(true) | FieldValue::Radio(true)
                        );
                        if *checked != previous {
                            if matches!(value, FieldValue::Radio(false)) {
                                return Err(err("Select another radio option to clear this one"));
                            }
                            bindings.FORM_OnLButtonDown(native.form, native.page, 0, x, y);
                            bindings.FORM_OnLButtonUp(native.form, native.page, 0, x, y);
                        }
                    }
                    FieldValue::Choice {
                        selected: choice, ..
                    } => {
                        let FieldValue::Choice { options, .. } = &actual.value else {
                            unreachable!()
                        };
                        let index = choice
                            .filter(|i| *i < options.len())
                            .ok_or_else(|| err("Choose a listed option"))?;
                        if bindings.FORM_SetIndexSelected(native.form, native.page, index as i32, 1)
                            == 0
                        {
                            return Err(err("PDFium could not select this option"));
                        }
                    }
                }
                bindings.FORM_ForceToKillFocus(native.form);
            }
            let bytes = native.save()?;
            drop(native);
            let candidate = self.candidate(doc, parse(&bytes)?)?;
            let result = self.form_fields(&candidate)?;
            let after = result
                .iter()
                .find(|f| f.page == actual.page && f.index == actual.index)
                .ok_or_else(|| err("Field disappeared"))?;
            let verified = match (&after.value, value) {
                (
                    FieldValue::Choice { selected: a, .. },
                    FieldValue::Choice { selected: b, .. },
                ) => a == b,
                (a, b) => a == b,
            };
            if !verified {
                return Err(err(
                    "Form value failed round-trip verification; document unchanged",
                ));
            }
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
struct NativeForm<'a> {
    bindings: &'a dyn PdfiumLibraryBindings,
    document: FPDF_DOCUMENT,
    form: FPDF_FORMHANDLE,
    page: FPDF_PAGE,
    _info: std::pin::Pin<Box<FPDF_FORMFILLINFO>>,
    _bytes: std::marker::PhantomData<&'a [u8]>,
}
impl<'a> NativeForm<'a> {
    fn open(bindings: &'a dyn PdfiumLibraryBindings, bytes: &'a [u8], page: u16) -> Result<Self> {
        // Zero initializes optional C callbacks and pointers. Version 2 disables all actions.
        let mut info = Box::pin(unsafe { std::mem::zeroed::<FPDF_FORMFILLINFO>() });
        info.version = 2;
        info.xfa_disabled = 1;
        let mut native = Self {
            bindings,
            document: std::ptr::null_mut(),
            form: std::ptr::null_mut(),
            page: std::ptr::null_mut(),
            _info: info,
            _bytes: std::marker::PhantomData,
        };
        unsafe {
            native.document = bindings.FPDF_LoadMemDocument64(bytes, None);
            if native.document.is_null() {
                return Err(err("PDFium could not open the form"));
            }
            native.form =
                bindings.FPDFDOC_InitFormFillEnvironment(native.document, &mut *native._info);
            if native.form.is_null() {
                return Err(err("PDFium could not initialize the form"));
            }
            native.page = bindings.FPDF_LoadPage(native.document, i32::from(page));
            if native.page.is_null() {
                return Err(err("Invalid form page"));
            }
            bindings.FORM_OnAfterLoadPage(native.page, native.form);
        }
        Ok(native)
    }
    fn save(&mut self) -> Result<Vec<u8>> {
        #[repr(C)]
        struct Writer {
            interface: FPDF_FILEWRITE,
            bytes: Vec<u8>,
        }
        unsafe extern "C" fn write(
            writer: *mut FPDF_FILEWRITE,
            data: *const std::ffi::c_void,
            size: std::os::raw::c_ulong,
        ) -> i32 {
            let writer = unsafe { &mut *writer.cast::<Writer>() };
            if size as usize > MAX_FILE.saturating_sub(writer.bytes.len()) {
                return 0;
            }
            writer.bytes.extend_from_slice(unsafe {
                std::slice::from_raw_parts(data.cast(), size as usize)
            });
            1
        }
        let mut writer = Writer {
            interface: FPDF_FILEWRITE {
                version: 1,
                WriteBlock: Some(write),
            },
            bytes: vec![],
        };
        if unsafe {
            self.bindings
                .FPDF_SaveAsCopy(self.document, &mut writer.interface, 0)
        } == 0
        {
            return Err(err("Form could not be saved"));
        }
        Ok(writer.bytes)
    }
}
impl Drop for NativeForm<'_> {
    fn drop(&mut self) {
        unsafe {
            if !self.page.is_null() {
                self.bindings.FORM_OnBeforeClosePage(self.page, self.form);
                self.bindings.FPDF_ClosePage(self.page);
            }
            if !self.form.is_null() {
                self.bindings.FPDFDOC_ExitFormFillEnvironment(self.form);
            }
            if !self.document.is_null() {
                self.bindings.FPDF_CloseDocument(self.document);
            }
        }
    }
}
