#include <EGL/egl.h>
#include <GL/gl.h>
#include <stdbool.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wayland-client.h>
#include <wayland-egl.h>

#include "viewporter-client-protocol.h"
#include "presentation-time-client-protocol.h"

#define OKP_FEEDBACK_RING_CAPACITY 1024

enum okp_wayland_feedback_kind {
    OKP_WAYLAND_FEEDBACK_PRESENTED = 1,
    OKP_WAYLAND_FEEDBACK_DISCARDED = 2,
};

struct okp_wayland_feedback_record {
    uint32_t kind;
    uint64_t observed_monotonic_ns;
    uint64_t presented_ns;
    uint64_t sequence;
    uint32_t refresh_ns;
    uint32_t flags;
    int32_t width;
    int32_t height;
};

struct pending_feedback;

#ifndef EGL_CONTEXT_MAJOR_VERSION
#define EGL_CONTEXT_MAJOR_VERSION 0x3098
#endif
#ifndef EGL_CONTEXT_MINOR_VERSION
#define EGL_CONTEXT_MINOR_VERSION 0x30FB
#endif
#ifndef EGL_CONTEXT_OPENGL_PROFILE_MASK
#define EGL_CONTEXT_OPENGL_PROFILE_MASK 0x30FD
#endif
#ifndef EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT
#define EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT 0x00000001
#endif

struct okp_wayland_video_plane {
    struct wl_display *display;
    struct wl_compositor *compositor;
    struct wl_event_queue *queue;
    struct wl_registry *registry;
    struct wl_subcompositor *subcompositor;
    struct wp_viewporter *viewporter;
    struct wp_presentation *presentation;
    struct wl_surface *surface;
    struct wl_subsurface *subsurface;
    struct wp_viewport *viewport;
    struct wl_egl_window *egl_window;
    EGLDisplay egl_display;
    EGLContext egl_context;
    EGLSurface egl_surface;
    int logical_width;
    int logical_height;
    int buffer_width;
    int buffer_height;
    struct pending_feedback *pending_feedback;
    struct okp_wayland_feedback_record feedback_ring[OKP_FEEDBACK_RING_CAPACITY];
    atomic_size_t feedback_read;
    atomic_size_t feedback_write;
};

struct registry_state {
    struct wl_subcompositor *subcompositor;
    struct wp_viewporter *viewporter;
    struct wp_presentation *presentation;
};

struct pending_feedback {
    struct okp_wayland_video_plane *plane;
    struct wp_presentation_feedback *feedback;
    struct pending_feedback *next;
};

static void write_error(char *buffer, size_t length, const char *message) {
    if (buffer == NULL || length == 0) {
        return;
    }
    snprintf(buffer, length, "%s", message);
}

static void write_egl_error(char *buffer, size_t length, const char *operation) {
    if (buffer == NULL || length == 0) {
        return;
    }
    snprintf(buffer, length, "%s failed with EGL error 0x%04x", operation,
             (unsigned int)eglGetError());
}

static EGLContext create_mpv_style_context(EGLDisplay display, EGLConfig config) {
    static const EGLint versions[][3] = {
        {4, 4, EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT},
        {3, 2, EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT},
        {2, 1, 0},
    };
    for (size_t index = 0; index < sizeof(versions) / sizeof(versions[0]); index++) {
        EGLint attributes[] = {
            EGL_CONTEXT_MAJOR_VERSION, versions[index][0],
            EGL_CONTEXT_MINOR_VERSION, versions[index][1],
            EGL_CONTEXT_OPENGL_PROFILE_MASK, versions[index][2],
            EGL_NONE,
        };
        EGLContext context =
            eglCreateContext(display, config, EGL_NO_CONTEXT, attributes);
        if (context != EGL_NO_CONTEXT) {
            return context;
        }
    }
    return eglCreateContext(display, config, EGL_NO_CONTEXT,
                            (EGLint[]){EGL_NONE});
}

static void registry_global(void *data, struct wl_registry *registry, uint32_t name,
                            const char *interface, uint32_t version) {
    struct registry_state *state = data;
    if (strcmp(interface, wl_subcompositor_interface.name) == 0) {
        state->subcompositor = wl_registry_bind(
            registry, name, &wl_subcompositor_interface, version < 1 ? version : 1);
    } else if (strcmp(interface, wp_viewporter_interface.name) == 0) {
        state->viewporter = wl_registry_bind(
            registry, name, &wp_viewporter_interface, version < 1 ? version : 1);
    } else if (strcmp(interface, wp_presentation_interface.name) == 0) {
        state->presentation = wl_registry_bind(
            registry, name, &wp_presentation_interface, version < 1 ? version : 1);
    }
}

static void registry_global_remove(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data;
    (void)registry;
    (void)name;
}

static const struct wl_registry_listener registry_listener = {
    .global = registry_global,
    .global_remove = registry_global_remove,
};

static bool bind_wayland_globals(
    struct okp_wayland_video_plane *plane, char *error, size_t error_length) {
    struct registry_state state = {0};
    struct wl_proxy *display_wrapper = wl_proxy_create_wrapper(plane->display);
    if (display_wrapper == NULL) {
        write_error(error, error_length, "wl_proxy_create_wrapper failed");
        return false;
    }

    plane->queue = wl_display_create_queue(plane->display);
    if (plane->queue == NULL) {
        wl_proxy_wrapper_destroy(display_wrapper);
        write_error(error, error_length, "wl_display_create_queue failed");
        return false;
    }
    wl_proxy_set_queue(display_wrapper, plane->queue);
    plane->registry = wl_display_get_registry((struct wl_display *)display_wrapper);
    wl_proxy_wrapper_destroy(display_wrapper);
    if (plane->registry == NULL) {
        write_error(error, error_length, "wl_display_get_registry failed");
        return false;
    }
    wl_registry_add_listener(plane->registry, &registry_listener, &state);
    if (wl_display_roundtrip_queue(plane->display, plane->queue) < 0) {
        write_error(error, error_length, "Wayland registry roundtrip failed");
        return false;
    }
    if (state.subcompositor == NULL) {
        write_error(error, error_length, "the Wayland compositor has no wl_subcompositor");
        return false;
    }
    if (state.viewporter == NULL) {
        write_error(error, error_length, "the Wayland compositor has no wp_viewporter");
        return false;
    }
    plane->subcompositor = state.subcompositor;
    plane->viewporter = state.viewporter;
    plane->presentation = state.presentation;
    return true;
}

static void remove_pending_feedback(struct pending_feedback *pending) {
    struct pending_feedback **cursor = &pending->plane->pending_feedback;
    while (*cursor != NULL) {
        if (*cursor == pending) {
            *cursor = pending->next;
            return;
        }
        cursor = &(*cursor)->next;
    }
}

static void push_feedback(
    struct okp_wayland_video_plane *plane,
    struct okp_wayland_feedback_record record) {
    size_t write = atomic_load_explicit(&plane->feedback_write, memory_order_relaxed);
    size_t next = (write + 1) % OKP_FEEDBACK_RING_CAPACITY;
    size_t read = atomic_load_explicit(&plane->feedback_read, memory_order_acquire);
    if (next == read) {
        return;
    }
    plane->feedback_ring[write] = record;
    atomic_store_explicit(&plane->feedback_write, next, memory_order_release);
}

static uint64_t monotonic_ns(void) {
    struct timespec timestamp = {0};
    if (clock_gettime(CLOCK_MONOTONIC, &timestamp) != 0) {
        return 0;
    }
    return (uint64_t)timestamp.tv_sec * 1000000000ULL +
           (uint64_t)timestamp.tv_nsec;
}

static void feedback_sync_output(
    void *data, struct wp_presentation_feedback *feedback, struct wl_output *output) {
    (void)data;
    (void)feedback;
    (void)output;
}

static void feedback_presented(
    void *data, struct wp_presentation_feedback *feedback, uint32_t tv_sec_hi,
    uint32_t tv_sec_lo, uint32_t tv_nsec, uint32_t refresh_nsec,
    uint32_t seq_hi, uint32_t seq_lo, uint32_t flags) {
    struct pending_feedback *pending = data;
    struct okp_wayland_video_plane *plane = pending->plane;
    uint64_t seconds = ((uint64_t)tv_sec_hi << 32) | tv_sec_lo;
    push_feedback(plane, (struct okp_wayland_feedback_record) {
        .kind = OKP_WAYLAND_FEEDBACK_PRESENTED,
        .observed_monotonic_ns = monotonic_ns(),
        .presented_ns = seconds * 1000000000ULL + tv_nsec,
        .sequence = ((uint64_t)seq_hi << 32) | seq_lo,
        .refresh_ns = refresh_nsec,
        .flags = flags,
        .width = plane->buffer_width,
        .height = plane->buffer_height,
    });
    remove_pending_feedback(pending);
    wp_presentation_feedback_destroy(feedback);
    free(pending);
}

static void feedback_discarded(
    void *data, struct wp_presentation_feedback *feedback) {
    struct pending_feedback *pending = data;
    push_feedback(pending->plane, (struct okp_wayland_feedback_record) {
        .kind = OKP_WAYLAND_FEEDBACK_DISCARDED,
        .observed_monotonic_ns = monotonic_ns(),
    });
    remove_pending_feedback(pending);
    wp_presentation_feedback_destroy(feedback);
    free(pending);
}

static const struct wp_presentation_feedback_listener feedback_listener = {
    .sync_output = feedback_sync_output,
    .presented = feedback_presented,
    .discarded = feedback_discarded,
};

static void request_presentation_feedback(struct okp_wayland_video_plane *plane) {
    if (plane->presentation == NULL || getenv("OKP_PRESENT_LOG") == NULL) {
        return;
    }
    struct pending_feedback *pending = calloc(1, sizeof(*pending));
    if (pending == NULL) {
        return;
    }
    pending->plane = plane;
    pending->feedback = wp_presentation_feedback(plane->presentation, plane->surface);
    if (pending->feedback == NULL) {
        free(pending);
        return;
    }
    wl_proxy_set_queue((struct wl_proxy *)pending->feedback, plane->queue);
    pending->next = plane->pending_feedback;
    plane->pending_feedback = pending;
    wp_presentation_feedback_add_listener(
        pending->feedback, &feedback_listener, pending);
}

static void set_regions(struct okp_wayland_video_plane *plane) {
    struct wl_region *input = wl_compositor_create_region(plane->compositor);
    if (input != NULL) {
        wl_surface_set_input_region(plane->surface, input);
        wl_region_destroy(input);
    }

    struct wl_region *opaque = wl_compositor_create_region(plane->compositor);
    if (opaque != NULL) {
        wl_region_add(opaque, 0, 0, plane->logical_width, plane->logical_height);
        wl_surface_set_opaque_region(plane->surface, opaque);
        wl_region_destroy(opaque);
    }
}

static void destroy_plane(struct okp_wayland_video_plane *plane) {
    if (plane == NULL) {
        return;
    }
    if (plane->egl_display != EGL_NO_DISPLAY) {
        eglMakeCurrent(plane->egl_display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
        if (plane->egl_surface != EGL_NO_SURFACE) {
            eglDestroySurface(plane->egl_display, plane->egl_surface);
        }
        if (plane->egl_context != EGL_NO_CONTEXT) {
            eglDestroyContext(plane->egl_display, plane->egl_context);
        }
    }
    if (plane->egl_window != NULL) {
        wl_egl_window_destroy(plane->egl_window);
    }
    if (plane->viewport != NULL) {
        wp_viewport_destroy(plane->viewport);
    }
    if (plane->subsurface != NULL) {
        wl_subsurface_destroy(plane->subsurface);
    }
    if (plane->surface != NULL) {
        wl_surface_destroy(plane->surface);
    }
    if (plane->subcompositor != NULL) {
        wl_subcompositor_destroy(plane->subcompositor);
    }
    if (plane->viewporter != NULL) {
        wp_viewporter_destroy(plane->viewporter);
    }
    while (plane->pending_feedback != NULL) {
        struct pending_feedback *pending = plane->pending_feedback;
        plane->pending_feedback = pending->next;
        wp_presentation_feedback_destroy(pending->feedback);
        free(pending);
    }
    if (plane->presentation != NULL) {
        wp_presentation_destroy(plane->presentation);
    }
    if (plane->registry != NULL) {
        wl_registry_destroy(plane->registry);
    }
    if (plane->queue != NULL) {
        wl_event_queue_destroy(plane->queue);
    }
    free(plane);
}

struct okp_wayland_video_plane *okp_wayland_video_plane_create(
    void *display_pointer, void *compositor_pointer, void *parent_surface_pointer,
    void *egl_display_pointer, int logical_width, int logical_height,
    int buffer_width, int buffer_height, char *error, size_t error_length) {
    if (display_pointer == NULL || compositor_pointer == NULL ||
        parent_surface_pointer == NULL || egl_display_pointer == NULL) {
        write_error(error, error_length, "GDK did not expose the required Wayland/EGL handles");
        return NULL;
    }

    struct okp_wayland_video_plane *plane = calloc(1, sizeof(*plane));
    if (plane == NULL) {
        write_error(error, error_length, "allocating the native video plane failed");
        return NULL;
    }
    plane->display = display_pointer;
    plane->compositor = compositor_pointer;
    plane->egl_display = (EGLDisplay)egl_display_pointer;
    plane->egl_context = EGL_NO_CONTEXT;
    plane->egl_surface = EGL_NO_SURFACE;
    plane->logical_width = logical_width > 0 ? logical_width : 1;
    plane->logical_height = logical_height > 0 ? logical_height : 1;
    plane->buffer_width = buffer_width > 0 ? buffer_width : 1;
    plane->buffer_height = buffer_height > 0 ? buffer_height : 1;
    atomic_init(&plane->feedback_read, 0);
    atomic_init(&plane->feedback_write, 0);

    if (!bind_wayland_globals(plane, error, error_length)) {
        destroy_plane(plane);
        return NULL;
    }
    plane->surface = wl_compositor_create_surface(plane->compositor);
    if (plane->surface == NULL) {
        write_error(error, error_length, "wl_compositor_create_surface failed");
        destroy_plane(plane);
        return NULL;
    }
    plane->viewport = wp_viewporter_get_viewport(plane->viewporter, plane->surface);
    if (plane->viewport == NULL) {
        write_error(error, error_length, "wp_viewporter_get_viewport failed");
        destroy_plane(plane);
        return NULL;
    }
    plane->subsurface = wl_subcompositor_get_subsurface(
        plane->subcompositor, plane->surface, parent_surface_pointer);
    if (plane->subsurface == NULL) {
        write_error(error, error_length, "wl_subcompositor_get_subsurface failed");
        destroy_plane(plane);
        return NULL;
    }
    wl_subsurface_set_position(plane->subsurface, 0, 0);
    wl_subsurface_place_below(plane->subsurface, parent_surface_pointer);
    // The video plane must commit independently of GTK's GSK/frame-clock path.
    wl_subsurface_set_desync(plane->subsurface);
    // A viewport lets the subsurface use GTK's exact fractional scale instead
    // of rounding 150% or 175% up to the integer buffer scale.
    wl_surface_set_buffer_scale(plane->surface, 1);
    wp_viewport_set_destination(
        plane->viewport, plane->logical_width, plane->logical_height);
    set_regions(plane);

    EGLint config_attributes[] = {
        EGL_SURFACE_TYPE, EGL_WINDOW_BIT,
        EGL_RENDERABLE_TYPE, EGL_OPENGL_BIT,
        EGL_RED_SIZE, 8,
        EGL_GREEN_SIZE, 8,
        EGL_BLUE_SIZE, 8,
        EGL_ALPHA_SIZE, 0,
        EGL_NONE,
    };
    EGLConfig config = NULL;
    EGLint config_count = 0;
    if (!eglBindAPI(EGL_OPENGL_API)) {
        write_egl_error(error, error_length, "eglBindAPI");
        destroy_plane(plane);
        return NULL;
    }
    if (!eglChooseConfig(plane->egl_display, config_attributes, &config, 1,
                         &config_count) || config_count < 1) {
        write_egl_error(error, error_length, "eglChooseConfig");
        destroy_plane(plane);
        return NULL;
    }
    plane->egl_context = create_mpv_style_context(plane->egl_display, config);
    if (plane->egl_context == EGL_NO_CONTEXT) {
        write_egl_error(error, error_length, "eglCreateContext");
        destroy_plane(plane);
        return NULL;
    }

    plane->egl_window = wl_egl_window_create(
        plane->surface, plane->buffer_width, plane->buffer_height);
    if (plane->egl_window == NULL) {
        write_error(error, error_length, "wl_egl_window_create failed");
        destroy_plane(plane);
        return NULL;
    }
    plane->egl_surface = eglCreateWindowSurface(
        plane->egl_display, config, (EGLNativeWindowType)plane->egl_window, NULL);
    if (plane->egl_surface == EGL_NO_SURFACE) {
        write_egl_error(error, error_length, "eglCreateWindowSurface");
        destroy_plane(plane);
        return NULL;
    }
    if (!eglMakeCurrent(plane->egl_display, plane->egl_surface, plane->egl_surface,
                        plane->egl_context)) {
        write_egl_error(error, error_length, "eglMakeCurrent");
        destroy_plane(plane);
        return NULL;
    }
    if (getenv("OKP_DEBUG_WINDOW_FIT") != NULL) {
        fprintf(stderr, "native GL context: version=%s vendor=%s renderer=%s\n",
                (const char *)glGetString(GL_VERSION),
                (const char *)glGetString(GL_VENDOR),
                (const char *)glGetString(GL_RENDERER));
    }
    /* Match mpv's native Wayland OpenGL context. Blocking EGL presentation
     * misses the next vblank once a 4K render consumes part of the frame
     * budget; mpv uses interval 0 and performs Wayland frame pacing outside
     * EGL instead. libmpv's update callback provides the first-stage pacing
     * for this embedded surface. */
    if (!eglSwapInterval(plane->egl_display, 0)) {
        write_egl_error(error, error_length, "eglSwapInterval");
        destroy_plane(plane);
        return NULL;
    }

    glViewport(0, 0, plane->buffer_width, plane->buffer_height);
    glClearColor(0.0f, 0.0f, 0.0f, 1.0f);
    glClear(GL_COLOR_BUFFER_BIT);
    if (!eglSwapBuffers(plane->egl_display, plane->egl_surface)) {
        write_egl_error(error, error_length, "initial eglSwapBuffers");
        destroy_plane(plane);
        return NULL;
    }
    wl_display_flush(plane->display);
    return plane;
}

void okp_wayland_video_plane_destroy(struct okp_wayland_video_plane *plane) {
    destroy_plane(plane);
}

bool okp_wayland_video_plane_make_current(struct okp_wayland_video_plane *plane) {
    return plane != NULL &&
           eglMakeCurrent(plane->egl_display, plane->egl_surface, plane->egl_surface,
                          plane->egl_context);
}

bool okp_wayland_video_plane_release_current(struct okp_wayland_video_plane *plane) {
    return plane != NULL &&
           eglMakeCurrent(plane->egl_display, EGL_NO_SURFACE, EGL_NO_SURFACE,
                          EGL_NO_CONTEXT);
}

bool okp_wayland_video_plane_swap(struct okp_wayland_video_plane *plane) {
    if (plane == NULL) {
        return false;
    }
    request_presentation_feedback(plane);
    bool swapped = eglSwapBuffers(plane->egl_display, plane->egl_surface);
    wl_display_dispatch_queue_pending(plane->display, plane->queue);
    return swapped;
}

bool okp_wayland_video_plane_take_feedback(
    struct okp_wayland_video_plane *plane,
    struct okp_wayland_feedback_record *record) {
    if (plane == NULL || record == NULL) {
        return false;
    }
    size_t read = atomic_load_explicit(&plane->feedback_read, memory_order_relaxed);
    size_t write = atomic_load_explicit(&plane->feedback_write, memory_order_acquire);
    if (read == write) {
        return false;
    }
    *record = plane->feedback_ring[read];
    atomic_store_explicit(
        &plane->feedback_read,
        (read + 1) % OKP_FEEDBACK_RING_CAPACITY,
        memory_order_release);
    return true;
}

void okp_wayland_video_plane_resize(struct okp_wayland_video_plane *plane, int width,
                                    int height, int buffer_width, int buffer_height) {
    if (plane == NULL) {
        return;
    }
    plane->logical_width = width > 0 ? width : 1;
    plane->logical_height = height > 0 ? height : 1;
    plane->buffer_width = buffer_width > 0 ? buffer_width : 1;
    plane->buffer_height = buffer_height > 0 ? buffer_height : 1;
    wp_viewport_set_destination(
        plane->viewport, plane->logical_width, plane->logical_height);
    wl_egl_window_resize(
        plane->egl_window, plane->buffer_width, plane->buffer_height, 0, 0);
    set_regions(plane);
    if (getenv("OKP_DEBUG_WINDOW_FIT") != NULL) {
        fprintf(stderr,
                "native video geometry applied: surface=%dx%d+0,0 "
                "subsurface=%dx%d+0,0 buffer=%dx%d\n",
                plane->logical_width, plane->logical_height,
                plane->logical_width, plane->logical_height,
                plane->buffer_width, plane->buffer_height);
    }
}
