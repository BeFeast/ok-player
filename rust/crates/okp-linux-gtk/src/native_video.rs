use super::*;

use std::ffi::{CStr, c_char, c_void};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

#[repr(C)]
struct NativePlaneOpaque {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn okp_wayland_video_plane_create(
        display: *mut c_void,
        compositor: *mut c_void,
        parent_surface: *mut c_void,
        egl_display: *mut c_void,
        width: i32,
        height: i32,
        scale: i32,
        error: *mut c_char,
        error_length: usize,
    ) -> *mut NativePlaneOpaque;
    fn okp_wayland_video_plane_destroy(plane: *mut NativePlaneOpaque);
    fn okp_wayland_video_plane_make_current(plane: *mut NativePlaneOpaque) -> bool;
    fn okp_wayland_video_plane_release_current(plane: *mut NativePlaneOpaque) -> bool;
    fn okp_wayland_video_plane_swap(plane: *mut NativePlaneOpaque) -> bool;
    fn okp_wayland_video_plane_resize(
        plane: *mut NativePlaneOpaque,
        width: i32,
        height: i32,
        scale: i32,
    );
}

type GetDisplayHandle = unsafe extern "C" fn(*mut gdk::ffi::GdkDisplay) -> *mut c_void;
type GetSurfaceHandle = unsafe extern "C" fn(*mut gdk::ffi::GdkSurface) -> *mut c_void;

pub(crate) struct NativeVideoPlane {
    pointer: NonNull<NativePlaneOpaque>,
    width: AtomicI32,
    height: AtomicI32,
    scale: AtomicI32,
    applied_width: AtomicI32,
    applied_height: AtomicI32,
    applied_scale: AtomicI32,
    alive: AtomicBool,
}

// SAFETY: after creation releases the EGL context from GTK's thread, all native
// resize/render/swap calls run on the dedicated render thread. Teardown joins
// that thread before GTK makes the context current again and destroys it.
unsafe impl Send for NativeVideoPlane {}
unsafe impl Sync for NativeVideoPlane {}

impl NativeVideoPlane {
    pub(crate) fn create(widget: &impl IsA<gtk::Widget>) -> Result<Arc<Self>, String> {
        use gtk::glib::translate::ToGlibPtr;

        let display = widget.display();
        if !is_wayland_display(display.type_().name()) {
            return Err("the active GDK display is not Wayland".to_owned());
        }
        let native = widget
            .native()
            .ok_or_else(|| "the video host has no GtkNative root".to_owned())?;
        let surface = native
            .surface()
            .ok_or_else(|| "the GTK window has no realized GDK surface".to_owned())?;

        let get_wl_display = resolve_display_symbol(c"gdk_wayland_display_get_wl_display")?;
        let get_wl_compositor = resolve_display_symbol(c"gdk_wayland_display_get_wl_compositor")?;
        let get_egl_display = resolve_display_symbol(c"gdk_wayland_display_get_egl_display")?;
        let get_wl_surface = resolve_surface_symbol(c"gdk_wayland_surface_get_wl_surface")?;

        let display_pointer = unsafe { get_wl_display(display.to_glib_none().0) };
        let compositor_pointer = unsafe { get_wl_compositor(display.to_glib_none().0) };
        let egl_display_pointer = unsafe { get_egl_display(display.to_glib_none().0) };
        let parent_surface_pointer = unsafe { get_wl_surface(surface.to_glib_none().0) };
        let width = widget.width().max(1);
        let height = widget.height().max(1);
        let scale = widget.scale_factor().max(1);
        let mut error = [0_i8; 256];
        let pointer = NonNull::new(unsafe {
            okp_wayland_video_plane_create(
                display_pointer,
                compositor_pointer,
                parent_surface_pointer,
                egl_display_pointer,
                width,
                height,
                scale,
                error.as_mut_ptr(),
                error.len(),
            )
        })
        .ok_or_else(|| c_error(&error))?;

        Ok(Arc::new(Self {
            pointer,
            width: AtomicI32::new(width),
            height: AtomicI32::new(height),
            scale: AtomicI32::new(scale),
            applied_width: AtomicI32::new(width),
            applied_height: AtomicI32::new(height),
            applied_scale: AtomicI32::new(scale),
            alive: AtomicBool::new(true),
        }))
    }

    pub(crate) fn make_current(&self) -> bool {
        self.alive.load(Ordering::Acquire)
            && unsafe { okp_wayland_video_plane_make_current(self.pointer.as_ptr()) }
    }

    pub(crate) fn release_current(&self) -> bool {
        unsafe { okp_wayland_video_plane_release_current(self.pointer.as_ptr()) }
    }

    pub(crate) fn swap(&self) -> bool {
        self.alive.load(Ordering::Acquire)
            && unsafe { okp_wayland_video_plane_swap(self.pointer.as_ptr()) }
    }

    pub(crate) fn resize(&self, width: i32, height: i32, scale: i32) {
        if !self.alive.load(Ordering::Acquire) {
            return;
        }
        let width = width.max(1);
        let height = height.max(1);
        let scale = scale.max(1);
        self.width.store(width, Ordering::Release);
        self.height.store(height, Ordering::Release);
        self.scale.store(scale, Ordering::Release);
    }

    pub(crate) fn prepare_frame(&self) -> Option<okp_mpv::RenderTargetSize> {
        if !self.alive.load(Ordering::Acquire) {
            return None;
        }
        let width = self.width.load(Ordering::Acquire);
        let height = self.height.load(Ordering::Acquire);
        let scale = self.scale.load(Ordering::Acquire);
        let changed = self.applied_width.swap(width, Ordering::AcqRel) != width
            || self.applied_height.swap(height, Ordering::AcqRel) != height
            || self.applied_scale.swap(scale, Ordering::AcqRel) != scale;
        if changed {
            unsafe {
                okp_wayland_video_plane_resize(self.pointer.as_ptr(), width, height, scale);
            }
        }
        Some(okp_mpv::RenderTargetSize {
            width: width * scale,
            height: height * scale,
        })
    }

    pub(crate) fn disable(&self) {
        self.alive.store(false, Ordering::Release);
    }
}

impl Drop for NativeVideoPlane {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        unsafe {
            okp_wayland_video_plane_destroy(self.pointer.as_ptr());
        }
    }
}

fn resolve_display_symbol(name: &CStr) -> Result<GetDisplayHandle, String> {
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    if symbol.is_null() {
        return Err(format!("{} is unavailable", name.to_string_lossy()));
    }
    Ok(unsafe { std::mem::transmute::<*mut c_void, GetDisplayHandle>(symbol) })
}

fn resolve_surface_symbol(name: &CStr) -> Result<GetSurfaceHandle, String> {
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    if symbol.is_null() {
        return Err(format!("{} is unavailable", name.to_string_lossy()));
    }
    Ok(unsafe { std::mem::transmute::<*mut c_void, GetSurfaceHandle>(symbol) })
}

fn c_error(buffer: &[c_char]) -> String {
    unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_render_size_applies_the_wayland_buffer_scale() {
        let plane = NativeVideoPlane {
            pointer: NonNull::dangling(),
            width: AtomicI32::new(1708),
            height: AtomicI32::new(961),
            scale: AtomicI32::new(2),
            applied_width: AtomicI32::new(1708),
            applied_height: AtomicI32::new(961),
            applied_scale: AtomicI32::new(2),
            alive: AtomicBool::new(true),
        };
        assert_eq!(
            plane.prepare_frame(),
            Some(okp_mpv::RenderTargetSize {
                width: 3416,
                height: 1922
            })
        );
        std::mem::forget(plane);
    }
}
