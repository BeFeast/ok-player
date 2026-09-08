// Exercise the actual production protocol-owner lifecycle with stubbed wire
// boundaries. Real GTK/Wayland callback mapping is covered by the GUI fixture.
#include <assert.h>
#include <stdlib.h>
#include <unistd.h>
#include <wayland-client.h>
#include "presentation-time-client-protocol.h"
#include "viewporter-client-protocol.h"

static int requests, destroyed, registries, callbacks;
static char registry_object, presentation_object, sync_object, surface_object;
static struct wl_registry *test_registry(struct wl_display *display) {
    (void)display; registries++; return (void *)&registry_object;
}
static struct wl_callback *test_sync(struct wl_display *display) {
    (void)display; callbacks++; return (void *)&sync_object;
}
static void *test_bind(struct wl_registry *registry, uint32_t name,
                       const struct wl_interface *interface, uint32_t version) {
    (void)registry; (void)name; (void)interface; (void)version;
    return &presentation_object;
}
static struct wp_presentation_feedback *test_feedback(struct wp_presentation *p,
                                                      struct wl_surface *s) {
    (void)p; (void)s; requests++; return malloc(1);
}
static void test_destroy_feedback(struct wp_presentation_feedback *p) {
    destroyed++; free(p);
}
#define wl_display_get_registry test_registry
#define wl_display_sync test_sync
#define wl_registry_bind test_bind
#define wl_registry_add_listener(...) 0
#define wl_callback_add_listener(...) 0
#define wp_presentation_add_listener(...) 0
#define wp_presentation_feedback_add_listener(...) 0
#define wp_presentation_feedback(p, s) test_feedback(p, s)
#define wp_presentation_feedback_destroy test_destroy_feedback
#define wl_registry_destroy(...) ((void)0)
#define wl_callback_destroy(...) ((void)0)
#define wp_presentation_destroy(...) ((void)0)
#define wl_proxy_get_id(...) 17
#include "../../rust/crates/okp-linux-gtk/src/native_wayland_video.c"

int main(void) {
    alarm(5);
    struct wl_display *display = (void *)&sync_object;
    struct wl_surface *surface = (void *)&surface_object;
    unsetenv("OKP_PRESENT_LOG");
    assert(okp_gtk_timing_create(display, surface) == NULL);
    assert(registries == 0 && callbacks == 0 && requests == 0);
    setenv("OKP_PRESENT_LOG", "test", 1);
    struct okp_gtk_timing *owner = okp_gtk_timing_create(display, surface);
    assert(owner && registries == 1 && callbacks == 1);
    assert(okp_gtk_timing_request(owner, 1, 10) == 0);
    gtk_timing_global(owner, owner->registry, 3, "wp_presentation", 2);
    gtk_timing_clock(owner, owner->presentation, 4);
    assert(okp_gtk_timing_request(owner, 21, 100) == 1);
    assert(okp_gtk_timing_request(owner, 22, 100) == 2);
    assert(requests == 1);
    struct gtk_timing_pending *pending = owner->pending[0];
    gtk_timing_presented(pending, pending->feedback, 0, 12, 345, 16666666, 1, 7, 5);
    struct okp_gtk_timing_record record;
    assert(okp_gtk_timing_take(owner, &record) && record.kind == 3 && record.clock_id == 4);
    assert(okp_gtk_timing_take(owner, &record) && record.kind == 1);
    assert(record.render_sequence == 21 && record.frame_counter == 100);
    assert(record.presented_ns == 12000000345ULL && record.clock_id == 4);
    assert(record.output_sequence == ((1ULL << 32) | 7) && record.surface_id == 17);
    assert(okp_gtk_timing_request(owner, 23, 101) == 1);
    pending = owner->pending[0];
    gtk_timing_discarded(pending, pending->feedback);
    assert(okp_gtk_timing_take(owner, &record) && record.kind == 2);
    assert(record.render_sequence == 23 && record.frame_counter == 101);
    for (int i = 0; i < GTK_TIMING_CAPACITY; i++)
        assert(okp_gtk_timing_request(owner, 30 + i, 200 + i) == 1);
    assert(okp_gtk_timing_request(owner, 99, 999) == 3);
    assert(okp_gtk_timing_destroy(owner) == GTK_TIMING_CAPACITY);
    assert(destroyed == requests);
    owner = okp_gtk_timing_create(display, surface);
    gtk_timing_discovered(owner, owner->discovery, 0);
    assert(okp_gtk_timing_request(owner, 1, 1) == 4);
    assert(okp_gtk_timing_take(owner, &record) && record.kind == 4);
    assert(okp_gtk_timing_destroy(owner) == 0);
    puts("PASS actual GTK feedback owner: disabled, correlation, RAW clock, discard, bounds, cleanup");
    return 0;
}
