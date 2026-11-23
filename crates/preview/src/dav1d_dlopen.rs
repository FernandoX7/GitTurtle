//! Isolated runtime `dlopen` wrapper for the system `libdav1d.so.7` (API major 7)
//! inside GitTurtle's confined Linux helper process.
//!
//! By loading `libdav1d.so.7` dynamically before `RLIMIT_NOFILE` / seccomp
//! confinement rather than hard-linking `DT_NEEDED` onto the main binary:
//! - GitTurtle launches cleanly on minimal Linux systems without `libdav1d`.
//! - Only the 10 reviewed decoding symbols are resolved.
//! - `DAV1D_API_MAJOR(dav1d_version_api()) == 7` is verified before any C ABI 7
//!   struct is touched, preventing ABI mismatch against unknown future SONAMEs.
//! - `crates/preview/src/avif.rs` stays `#![forbid(unsafe_code)]`.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::marker::PhantomData;
use std::ptr::{self, NonNull};
use std::sync::OnceLock;

/// Exact C ABI 7 layouts matching `<dav1d/dav1d.h>`, `<dav1d/picture.h>`,
/// `<dav1d/data.h>`, `<dav1d/common.h>`, and `<dav1d/headers.h>`.
/// Verified against the installed C headers by
/// `avif::tests::libdav1d_layouts_match_the_binding`.
pub(crate) mod abi {
    use std::ffi::{c_char, c_int, c_uint, c_void};

    pub const DAV1D_API_VERSION_MAJOR: u32 = 7;
    pub const DAV1D_MAX_OPERATING_POINTS: usize = 32;

    pub const DAV1D_PIXEL_LAYOUT_I400: c_uint = 0;
    pub const DAV1D_PIXEL_LAYOUT_I420: c_uint = 1;
    pub const DAV1D_PIXEL_LAYOUT_I422: c_uint = 2;
    pub const DAV1D_PIXEL_LAYOUT_I444: c_uint = 3;

    #[repr(C)]
    pub struct Dav1dContext {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct Dav1dRef {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct Dav1dFrameHeader {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct Dav1dContentLightLevel {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct Dav1dMasteringDisplay {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct Dav1dITUTT35 {
        _opaque: [u8; 0],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dUserData {
        pub data: *const u8,
        pub ref_: *mut Dav1dRef,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dDataProps {
        pub timestamp: i64,
        pub duration: i64,
        pub offset: i64,
        pub size: usize,
        pub user_data: Dav1dUserData,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dData {
        pub data: *const u8,
        pub sz: usize,
        pub ref_: *mut Dav1dRef,
        pub m: Dav1dDataProps,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dPicAllocator {
        pub cookie: *mut c_void,
        pub alloc_picture_callback:
            Option<unsafe extern "C" fn(*mut Dav1dPicture, *mut c_void) -> c_int>,
        pub release_picture_callback: Option<unsafe extern "C" fn(*mut Dav1dPicture, *mut c_void)>,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dLogger {
        pub cookie: *mut c_void,
        // C signature: `void (*callback)(void *cookie, const char *format, va_list ap)`.
        // Function pointers share identical size/alignment on Linux LP64; GitTurtle
        // leaves dav1d's default logger untouched and never calls or replaces it.
        pub callback: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void)>,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dSettings {
        pub n_threads: c_int,
        pub max_frame_delay: c_int,
        pub apply_grain: c_int,
        pub operating_point: c_int,
        pub all_layers: c_int,
        pub frame_size_limit: c_uint,
        pub allocator: Dav1dPicAllocator,
        pub logger: Dav1dLogger,
        pub strict_std_compliance: c_int,
        pub output_invisible_frames: c_int,
        pub inloop_filters: c_uint,
        pub decode_frame_type: c_uint,
        pub reserved: [u8; 16],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dPictureParameters {
        pub w: c_int,
        pub h: c_int,
        pub layout: c_uint,
        pub bpc: c_int,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dPicture {
        pub seq_hdr: *mut Dav1dSequenceHeader,
        pub frame_hdr: *mut Dav1dFrameHeader,
        pub data: [*mut c_void; 3],
        pub stride: [isize; 2],
        pub p: Dav1dPictureParameters,
        pub m: Dav1dDataProps,
        pub content_light: *mut Dav1dContentLightLevel,
        pub mastering_display: *mut Dav1dMasteringDisplay,
        pub itut_t35: *mut Dav1dITUTT35,
        pub n_itut_t35: usize,
        pub reserved: [usize; 4],
        pub frame_hdr_ref: *mut Dav1dRef,
        pub seq_hdr_ref: *mut Dav1dRef,
        pub content_light_ref: *mut Dav1dRef,
        pub mastering_display_ref: *mut Dav1dRef,
        pub itut_t35_ref: *mut Dav1dRef,
        pub reserved_ref: [usize; 4],
        pub ref_: *mut Dav1dRef,
        pub allocator_data: *mut c_void,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dSequenceHeaderOperatingPoint {
        pub major_level: u8,
        pub minor_level: u8,
        pub initial_display_delay: u8,
        pub idc: u16,
        pub tier: u8,
        pub decoder_model_param_present: u8,
        pub display_model_param_present: u8,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dSequenceHeaderOperatingParameterInfo {
        pub decoder_buffer_delay: u32,
        pub encoder_buffer_delay: u32,
        pub low_delay_mode: u8,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Dav1dSequenceHeader {
        pub profile: u8,
        pub max_width: c_int,
        pub max_height: c_int,
        pub layout: c_uint,
        pub pri: c_uint,
        pub trc: c_uint,
        pub mtrx: c_uint,
        pub chr: c_uint,
        pub hbd: u8,
        pub color_range: u8,
        pub num_operating_points: u8,
        pub operating_points: [Dav1dSequenceHeaderOperatingPoint; DAV1D_MAX_OPERATING_POINTS],
        pub still_picture: u8,
        pub reduced_still_picture_header: u8,
        pub timing_info_present: u8,
        pub num_units_in_tick: u32,
        pub time_scale: u32,
        pub equal_picture_interval: u8,
        pub num_ticks_per_picture: u32,
        pub decoder_model_info_present: u8,
        pub encoder_decoder_buffer_delay_length: u8,
        pub num_units_in_decoding_tick: u32,
        pub buffer_removal_delay_length: u8,
        pub frame_presentation_delay_length: u8,
        pub display_model_info_present: u8,
        pub width_n_bits: u8,
        pub height_n_bits: u8,
        pub frame_id_numbers_present: u8,
        pub delta_frame_id_n_bits: u8,
        pub frame_id_n_bits: u8,
        pub sb128: u8,
        pub filter_intra: u8,
        pub intra_edge_filter: u8,
        pub inter_intra: u8,
        pub masked_compound: u8,
        pub warped_motion: u8,
        pub dual_filter: u8,
        pub order_hint: u8,
        pub jnt_comp: u8,
        pub ref_frame_mvs: u8,
        pub screen_content_tools: c_uint,
        pub force_integer_mv: c_uint,
        pub order_hint_n_bits: u8,
        pub super_res: u8,
        pub cdef: u8,
        pub restoration: u8,
        pub ss_hor: u8,
        pub ss_ver: u8,
        pub monochrome: u8,
        pub color_description_present: u8,
        pub separate_uv_delta_q: u8,
        pub film_grain_present: u8,
        pub operating_parameter_info:
            [Dav1dSequenceHeaderOperatingParameterInfo; DAV1D_MAX_OPERATING_POINTS],
    }
}

use abi::*;

struct Dav1dApi {
    version: unsafe extern "C" fn() -> *const c_char,
    version_api: unsafe extern "C" fn() -> c_uint,
    default_settings: unsafe extern "C" fn(*mut Dav1dSettings),
    open: unsafe extern "C" fn(*mut *mut Dav1dContext, *const Dav1dSettings) -> c_int,
    close: unsafe extern "C" fn(*mut *mut Dav1dContext),
    data_create: unsafe extern "C" fn(*mut Dav1dData, usize) -> *mut u8,
    data_unref: unsafe extern "C" fn(*mut Dav1dData),
    send_data: unsafe extern "C" fn(*mut Dav1dContext, *mut Dav1dData) -> c_int,
    get_picture: unsafe extern "C" fn(*mut Dav1dContext, *mut Dav1dPicture) -> c_int,
    picture_unref: unsafe extern "C" fn(*mut Dav1dPicture),
}

// SAFETY: Every field is a plain C function pointer inside a process-lifetime
// shared library loaded via `dlopen` (and never `dlclose`d).
unsafe impl Send for Dav1dApi {}
// SAFETY: Function pointers in a process-lifetime shared object are thread-safe to invoke.
unsafe impl Sync for Dav1dApi {}

static API: OnceLock<Option<Dav1dApi>> = OnceLock::new();

/// Loads a required symbol from `handle` and transmutes it to `T`.
///
/// # Safety
/// `handle` must be a valid `dlopen` handle and `T` must exactly match the C
/// ABI signature of `name` in libdav1d API major 7.
unsafe fn load_sym<T: Copy>(handle: *mut c_void, name: &CStr) -> Option<T> {
    // SAFETY: Caller guarantees `handle` is valid and `name` is NUL-terminated.
    let ptr = unsafe { libc::dlsym(handle, name.as_ptr()) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: Caller guarantees `T` matches the symbol's C function pointer ABI.
    Some(unsafe { std::mem::transmute_copy(&ptr) })
}

fn load_api() -> Option<&'static Dav1dApi> {
    API.get_or_init(|| {
        // Probe libdav1d.so.7 (API major 7). Never dlclose so pointers remain valid
        // for the helper's lifetime and no teardown syscalls run under seccomp.
        // SAFETY: `c"libdav1d.so.7"` is a valid NUL-terminated SONAME string.
        let handle =
            unsafe { libc::dlopen(c"libdav1d.so.7".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return None;
        }
        // SAFETY: `handle` is non-null from `dlopen("libdav1d.so.7")` and every
        // signature matches `/usr/include/dav1d/*.h` for API major 7.
        let api = unsafe {
            Dav1dApi {
                version: load_sym(handle, c"dav1d_version")?,
                version_api: load_sym(handle, c"dav1d_version_api")?,
                default_settings: load_sym(handle, c"dav1d_default_settings")?,
                open: load_sym(handle, c"dav1d_open")?,
                close: load_sym(handle, c"dav1d_close")?,
                data_create: load_sym(handle, c"dav1d_data_create")?,
                data_unref: load_sym(handle, c"dav1d_data_unref")?,
                send_data: load_sym(handle, c"dav1d_send_data")?,
                get_picture: load_sym(handle, c"dav1d_get_picture")?,
                picture_unref: load_sym(handle, c"dav1d_picture_unref")?,
            }
        };
        // Verify runtime API major version is strictly 7 before touching any
        // API-7 struct layout.
        // SAFETY: `dav1d_version_api` takes no arguments and returns `0x00XXYYZZ`.
        let api_version = unsafe { (api.version_api)() };
        let major = (api_version >> 16) & 0xFF;
        if major != DAV1D_API_VERSION_MAJOR {
            return None;
        }
        Some(api)
    })
    .as_ref()
}

/// Preloads `libdav1d.so.7` before `RLIMIT_NOFILE` / seccomp confinement.
/// Missing or incompatible libraries are silently cached as unavailable so
/// PDF/JP2/3D requests continue to work without libdav1d installed.
pub(crate) fn preload() {
    let _ = load_api();
}

/// Returns the loaded libdav1d version string, or `None` if `libdav1d.so.7`
/// (API major 7) is unavailable.
pub(crate) fn version() -> Option<String> {
    let api = load_api()?;
    // SAFETY: `dav1d_version` returns a pointer to static read-only NUL-terminated text.
    let ptr = unsafe { (api.version)() };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: `ptr` is non-null and points to libdav1d's static version string.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::enum_variant_names,
    reason = "preserves the Error::UnknownError variant name expected by the AVIF decoder"
)]
pub(crate) enum Error {
    Again,
    NotEnoughMemory,
    InvalidArgument,
    Unavailable,
    UnknownError(i32),
}

impl Error {
    fn from_code(code: c_int) -> Self {
        if code == -libc::EAGAIN {
            Self::Again
        } else if code == -libc::ENOMEM {
            Self::NotEnoughMemory
        } else if code == -libc::EINVAL {
            Self::InvalidArgument
        } else {
            Self::UnknownError(code)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PixelLayout {
    I400,
    I420,
    I422,
    I444,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanarImageComponent {
    Y,
    U,
    V,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum YUVRange {
    Limited,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BitsPerComponent(pub usize);

pub(crate) struct Settings {
    raw: Dav1dSettings,
}

impl Settings {
    pub(crate) fn new() -> Result<Self, Error> {
        let api = load_api().ok_or(Error::Unavailable)?;
        // SAFETY: `Dav1dSettings` is a plain C struct initialized by `dav1d_default_settings`.
        let mut raw: Dav1dSettings = unsafe { std::mem::zeroed() };
        // SAFETY: `raw` points to valid writable `Dav1dSettings` memory matching API 7.
        unsafe { (api.default_settings)(&mut raw) };
        Ok(Self { raw })
    }

    pub(crate) fn set_n_threads(&mut self, n_threads: u32) {
        self.raw.n_threads = n_threads as c_int;
    }

    pub(crate) fn set_max_frame_delay(&mut self, max_frame_delay: u32) {
        self.raw.max_frame_delay = max_frame_delay as c_int;
    }

    pub(crate) fn set_frame_size_limit(&mut self, limit: u32) {
        self.raw.frame_size_limit = limit;
    }

    pub(crate) fn set_all_layers(&mut self, all_layers: bool) {
        self.raw.all_layers = c_int::from(all_layers);
    }

    pub(crate) fn set_operating_point(&mut self, operating_point: u32) {
        self.raw.operating_point = operating_point as c_int;
    }

    pub(crate) fn set_apply_grain(&mut self, apply_grain: bool) {
        self.raw.apply_grain = c_int::from(apply_grain);
    }

    pub(crate) fn set_output_invisible_frames(&mut self, output: bool) {
        self.raw.output_invisible_frames = c_int::from(output);
    }
}

pub(crate) struct Decoder {
    api: &'static Dav1dApi,
    ctx: NonNull<Dav1dContext>,
    pending: Option<Dav1dData>,
}

impl Decoder {
    pub(crate) fn with_settings(settings: &Settings) -> Result<Self, Error> {
        let api = load_api().ok_or(Error::Unavailable)?;
        let mut ctx: *mut Dav1dContext = ptr::null_mut();
        // SAFETY: `ctx` and `&settings.raw` are valid pointers for `dav1d_open`.
        let ret = unsafe { (api.open)(&mut ctx, &settings.raw) };
        if ret < 0 {
            return Err(Error::from_code(ret));
        }
        let ctx = NonNull::new(ctx).ok_or(Error::NotEnoughMemory)?;
        Ok(Self {
            api,
            ctx,
            pending: None,
        })
    }

    pub(crate) fn send_data(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.pending.is_some() {
            return Err(Error::Again);
        }
        // SAFETY: `Dav1dData` is zero-initialized before `dav1d_data_create`.
        let mut data: Dav1dData = unsafe { std::mem::zeroed() };
        // SAFETY: `dav1d_data_create` allocates `bytes.len()` writable bytes owned by `data`.
        let dst = unsafe { (self.api.data_create)(&mut data, bytes.len()) };
        if dst.is_null() {
            return Err(Error::NotEnoughMemory);
        }
        if !bytes.is_empty() {
            // SAFETY: `dst` points to `bytes.len()` freshly allocated non-overlapping bytes.
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len()) };
        }
        // SAFETY: `self.ctx` is an open decoder and `data` holds a valid allocated packet.
        let ret = unsafe { (self.api.send_data)(self.ctx.as_ptr(), &mut data) };
        if ret == 0 {
            Ok(())
        } else if ret == -libc::EAGAIN {
            self.pending = Some(data);
            Err(Error::Again)
        } else {
            // SAFETY: On error other than 0, caller still owns `data` and must unref it.
            unsafe { (self.api.data_unref)(&mut data) };
            Err(Error::from_code(ret))
        }
    }

    pub(crate) fn send_pending_data(&mut self) -> Result<(), Error> {
        let Some(mut data) = self.pending.take() else {
            return Ok(());
        };
        // SAFETY: `self.ctx` is an open decoder and `data` is our pending owned packet.
        let ret = unsafe { (self.api.send_data)(self.ctx.as_ptr(), &mut data) };
        if ret == 0 {
            Ok(())
        } else if ret == -libc::EAGAIN {
            self.pending = Some(data);
            Err(Error::Again)
        } else {
            // SAFETY: Caller still owns `data` on error and must release it.
            unsafe { (self.api.data_unref)(&mut data) };
            Err(Error::from_code(ret))
        }
    }

    pub(crate) fn get_picture(&mut self) -> Result<Picture, Error> {
        // SAFETY: `Dav1dPicture` must be zero-initialized before `dav1d_get_picture`.
        let mut raw: Dav1dPicture = unsafe { std::mem::zeroed() };
        // SAFETY: `self.ctx` is open and `&mut raw` is writable `Dav1dPicture` storage.
        let ret = unsafe { (self.api.get_picture)(self.ctx.as_ptr(), &mut raw) };
        if ret == 0 {
            Ok(Picture { api: self.api, raw })
        } else {
            Err(Error::from_code(ret))
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        if let Some(mut data) = self.pending.take() {
            // SAFETY: `data` is an owned unconsumed `Dav1dData`.
            unsafe { (self.api.data_unref)(&mut data) };
        }
        let mut ctx = self.ctx.as_ptr();
        // SAFETY: `ctx` was returned by `dav1d_open` and has not been closed yet.
        unsafe { (self.api.close)(&mut ctx) };
    }
}

pub(crate) struct Picture {
    api: &'static Dav1dApi,
    raw: Dav1dPicture,
}

/// Borrowed slice of one decoded plane, tied to the owning [`Picture`].
pub(crate) struct Plane<'a> {
    bytes: &'a [u8],
    _marker: PhantomData<&'a Picture>,
}

impl std::ops::Deref for Plane<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.bytes
    }
}

impl Picture {
    pub(crate) fn width(&self) -> u32 {
        u32::try_from(self.raw.p.w).unwrap_or(0)
    }

    pub(crate) fn height(&self) -> u32 {
        u32::try_from(self.raw.p.h).unwrap_or(0)
    }

    pub(crate) fn pixel_layout(&self) -> Option<PixelLayout> {
        match self.raw.p.layout {
            DAV1D_PIXEL_LAYOUT_I400 => Some(PixelLayout::I400),
            DAV1D_PIXEL_LAYOUT_I420 => Some(PixelLayout::I420),
            DAV1D_PIXEL_LAYOUT_I422 => Some(PixelLayout::I422),
            DAV1D_PIXEL_LAYOUT_I444 => Some(PixelLayout::I444),
            _ => None,
        }
    }

    pub(crate) fn bit_depth(&self) -> usize {
        usize::try_from(self.raw.p.bpc).unwrap_or(0)
    }

    pub(crate) fn bits_per_component(&self) -> Option<BitsPerComponent> {
        let seq = self.seq_hdr()?;
        match seq.hbd {
            0 => Some(BitsPerComponent(8)),
            1 => Some(BitsPerComponent(10)),
            2 => Some(BitsPerComponent(12)),
            _ => None,
        }
    }

    pub(crate) fn stride(&self, component: PlanarImageComponent) -> isize {
        match component {
            PlanarImageComponent::Y => self.raw.stride[0],
            PlanarImageComponent::U | PlanarImageComponent::V => self.raw.stride[1],
        }
    }

    pub(crate) fn plane(&self, component: PlanarImageComponent) -> Option<Plane<'_>> {
        let layout = self.pixel_layout()?;
        let index = match component {
            PlanarImageComponent::Y => 0,
            PlanarImageComponent::U => {
                if layout == PixelLayout::I400 {
                    return None;
                }
                1
            }
            PlanarImageComponent::V => {
                if layout == PixelLayout::I400 {
                    return None;
                }
                2
            }
        };
        let ptr = self.raw.data[index].cast::<u8>();
        if ptr.is_null() {
            return None;
        }
        let stride = usize::try_from(self.stride(component)).ok()?;
        let height = usize::try_from(self.raw.p.h).ok()?;
        let rows = match (component, layout) {
            (PlanarImageComponent::Y, _) | (_, PixelLayout::I444 | PixelLayout::I422) => height,
            (_, PixelLayout::I420) => height.div_ceil(2),
            (_, PixelLayout::I400) => return None,
        };
        let len = stride.checked_mul(rows)?;
        if len == 0 || isize::try_from(len).is_err() {
            return None;
        }
        // SAFETY: `dav1d_get_picture` guarantees `data[index]` holds `stride * rows`
        // readable bytes valid until `dav1d_picture_unref` in `Drop`.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        Some(Plane {
            bytes,
            _marker: PhantomData,
        })
    }

    fn seq_hdr(&self) -> Option<&Dav1dSequenceHeader> {
        // SAFETY: When non-null, `seq_hdr` points to a valid `Dav1dSequenceHeader`
        // refcounted by `self.raw` until `dav1d_picture_unref` in `Drop`.
        unsafe { self.raw.seq_hdr.as_ref() }
    }

    pub(crate) fn color_primaries(&self) -> u8 {
        self.seq_hdr()
            .and_then(|seq| u8::try_from(seq.pri).ok())
            .unwrap_or(2)
    }

    pub(crate) fn transfer_characteristic(&self) -> u8 {
        self.seq_hdr()
            .and_then(|seq| u8::try_from(seq.trc).ok())
            .unwrap_or(2)
    }

    pub(crate) fn matrix_coefficients(&self) -> u8 {
        self.seq_hdr()
            .and_then(|seq| u8::try_from(seq.mtrx).ok())
            .unwrap_or(2)
    }

    pub(crate) fn color_range(&self) -> YUVRange {
        match self.seq_hdr().map(|seq| seq.color_range) {
            Some(1) => YUVRange::Full,
            _ => YUVRange::Limited,
        }
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: `self.raw` was populated by `dav1d_get_picture` and is unreffed once.
        unsafe { (self.api.picture_unref)(&mut self.raw) };
    }
}
