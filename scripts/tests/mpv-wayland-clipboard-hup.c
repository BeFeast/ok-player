#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include <wayland-client.h>

#include "ext-data-control-v1.h"
#include "player/clipboard/clipboard.h"
#include "common/common.h"
#include "common/msg.h"
#include "misc/bstr.h"
#include "osdep/io.h"
#include "osdep/poll_wrapper.h"
#include "osdep/threads.h"

static int display_cancels;
static int display_disconnects;
static int offers_destroyed;
static const char *offered_payload;

static int test_display_prepare_read(struct wl_display *display)
{
    (void)display;
    return 0;
}

static int test_display_dispatch_pending(struct wl_display *display)
{
    (void)display;
    return 0;
}

static int test_display_flush(struct wl_display *display)
{
    (void)display;
    return 0;
}

static int test_display_read_events(struct wl_display *display)
{
    (void)display;
    return 0;
}

static void test_display_cancel_read(struct wl_display *display)
{
    (void)display;
    display_cancels++;
}

static void test_display_disconnect(struct wl_display *display)
{
    (void)display;
    display_disconnects++;
}

static void test_offer_destroy(struct ext_data_control_offer_v1 *offer)
{
    (void)offer;
    offers_destroyed++;
}

static void test_offer_receive(struct ext_data_control_offer_v1 *offer,
                               const char *mime_type, int32_t fd)
{
    (void)offer;
    (void)mime_type;
    if (!offered_payload)
        return;
    size_t size = strlen(offered_payload);
    if (write(fd, offered_payload, size) != (ssize_t)size)
        abort();
}

static int test_mp_poll(struct pollfd *fds, int nfds, int64_t timeout_ns)
{
    (void)timeout_ns;
    return poll(fds, nfds, 0);
}

static void *test_zero_alloc(size_t size)
{
    void *memory = calloc(1, size);
    if (!memory)
        abort();
    return memory;
}

static void *test_realloc(void *memory, size_t size)
{
    void *resized = realloc(memory, size);
    if (!resized)
        abort();
    return resized;
}

static void test_free(void *memory)
{
    free(memory);
}

#define mp_poll test_mp_poll
#define wl_display_prepare_read test_display_prepare_read
#define wl_display_dispatch_pending test_display_dispatch_pending
#define wl_display_flush test_display_flush
#define wl_display_read_events test_display_read_events
#define wl_display_cancel_read test_display_cancel_read
#define wl_display_disconnect test_display_disconnect
#define ext_data_control_offer_v1_destroy test_offer_destroy
#define ext_data_control_offer_v1_receive test_offer_receive

#undef talloc_zero_size
#undef talloc_realloc_size
#undef talloc_free
#define talloc_zero_size(ctx, size) test_zero_alloc(size)
#define talloc_realloc_size(ctx, ptr, size) test_realloc(ptr, size)
#define talloc_free(ptr) test_free(ptr)

#undef MP_VERBOSE
#undef MP_ERR
#undef MP_FATAL
#define MP_VERBOSE(obj, ...) ((void)0)
#define MP_ERR(obj, ...) ((void)0)
#define MP_FATAL(obj, ...) ((void)0)

// Compile the pinned tree's implementation into this test translation unit.
// The boundaries above replace only compositor/logging/allocation services;
// offer creation, pipe I/O, dispatch ordering, cleanup, and shutdown are the
// production functions under test.
#include "player/clipboard/clipboard-wayland.c"

static void fail(const char *message)
{
    fprintf(stderr, "FAIL: %s\n", message);
    exit(1);
}

static void require(bool condition, const char *message)
{
    if (!condition)
        fail(message);
}

static void init_priv(struct clipboard_wayland_priv *wl,
                      struct clipboard_wayland_data_offer *selection,
                      struct clipboard_wayland_data_offer *primary,
                      int display_fd, int death_fd)
{
    *selection = (struct clipboard_wayland_data_offer){.fd = -1};
    *primary = (struct clipboard_wayland_data_offer){.fd = -1};
    *wl = (struct clipboard_wayland_priv){
        .display_fd = display_fd,
        .display = (struct wl_display *)1,
        .selection_offer = selection,
        .primary_selection_offer = primary,
    };
    wl->death_pipe[0] = death_fd;
    wl->death_pipe[1] = -1;
    wl_list_init(&wl->seat_list);
    require(mp_mutex_init(&wl->lock) == 0, "mutex initialization failed");
}

static void test_unsupported_offer_is_retired(void)
{
    int display_pipe[2];
    int death_pipe[2];
    require(pipe(display_pipe) == 0, "display pipe creation failed");
    require(pipe(death_pipe) == 0, "death pipe creation failed");

    struct clipboard_wayland_priv wl;
    struct clipboard_wayland_data_offer selection;
    struct clipboard_wayland_data_offer primary;
    init_priv(&wl, &selection, &primary, display_pipe[0], death_pipe[0]);

    struct clipboard_wayland_seat seat = {.wl = &wl};
    int destroyed_before = offers_destroyed;
    handle_selection(&seat, NULL, (struct ext_data_control_offer_v1 *)1, &selection);
    require(selection.fd >= 0, "unsupported MIME offer did not create its read pipe");

    struct pollfd readiness = {.fd = selection.fd, .events = POLLIN};
    require(poll(&readiness, 1, 0) == 1, "closed offer was not poll-ready");
    require(readiness.revents & POLLHUP, "closed offer did not report POLLHUP");

    require(clipboard_wayland_dispatch_events(&wl, 0), "dispatch stopped unexpectedly");
    require(selection.fd == -1, "closed empty offer remained registered");
    require(offers_destroyed == destroyed_before + 1, "closed offer was not destroyed");
    require(clipboard_wayland_dispatch_events(&wl, 0), "second dispatch stopped unexpectedly");
    require(selection.fd == -1, "closed offer became ready again");

    mp_mutex_destroy(&wl.lock);
    close(display_pipe[0]);
    close(display_pipe[1]);
    close(death_pipe[0]);
    close(death_pipe[1]);
}

static void test_buffered_text_survives_hangup(void)
{
    static const char payload[] = "clipboard text retained";
    int display_pipe[2];
    int death_pipe[2];
    require(pipe(display_pipe) == 0, "display pipe creation failed");
    require(pipe(death_pipe) == 0, "death pipe creation failed");

    struct clipboard_wayland_priv wl;
    struct clipboard_wayland_data_offer selection;
    struct clipboard_wayland_data_offer primary;
    init_priv(&wl, &selection, &primary, display_pipe[0], death_pipe[0]);

    struct clipboard_wayland_seat seat = {.wl = &wl, .offered_plain_text = true};
    offered_payload = payload;
    handle_selection(&seat, NULL, (struct ext_data_control_offer_v1 *)1, &selection);
    offered_payload = NULL;

    struct pollfd readiness = {.fd = selection.fd, .events = POLLIN};
    require(poll(&readiness, 1, 0) == 1, "buffered offer was not poll-ready");
    require((readiness.revents & (POLLIN | POLLHUP)) == (POLLIN | POLLHUP),
            "fixture did not produce simultaneous POLLIN and POLLHUP");

    require(clipboard_wayland_dispatch_events(&wl, 0), "dispatch stopped unexpectedly");
    require(selection.fd == -1, "consumed offer remained registered");
    require(wl.data_changed, "clipboard data change was not published");
    require(wl.selection_text.len == sizeof(payload) - 1, "clipboard text length changed");
    require(memcmp(wl.selection_text.start, payload, sizeof(payload) - 1) == 0,
            "buffered clipboard text was discarded");

    talloc_free(wl.selection_text.start);
    mp_mutex_destroy(&wl.lock);
    close(display_pipe[0]);
    close(display_pipe[1]);
    close(death_pipe[0]);
    close(death_pipe[1]);
}

static void test_death_pipe_stops_and_cleans_up(void)
{
    int display_pipe[2];
    int death_pipe[2];
    require(pipe(display_pipe) == 0, "display pipe creation failed");
    require(pipe(death_pipe) == 0, "death pipe creation failed");
    require(write(death_pipe[1], "x", 1) == 1, "death signal write failed");

    struct clipboard_wayland_priv wl;
    struct clipboard_wayland_data_offer selection;
    struct clipboard_wayland_data_offer primary;
    init_priv(&wl, &selection, &primary, display_pipe[0], death_pipe[0]);

    int cancels_before = display_cancels;
    int disconnects_before = display_disconnects;
    clipboard_wayland_run(&wl);
    require(display_cancels == cancels_before + 1, "shutdown did not cancel prepared read");
    require(display_disconnects == disconnects_before + 1,
            "shutdown did not disconnect Wayland display");

    mp_mutex_destroy(&wl.lock);
    close(display_pipe[0]);
    close(display_pipe[1]);
    close(death_pipe[0]);
    close(death_pipe[1]);
}

int main(void)
{
    test_unsupported_offer_is_retired();
    test_buffered_text_survives_hangup();
    test_death_pipe_stops_and_cleans_up();
    puts("ok: actual mpv Wayland clipboard dispatch handles HUP, buffered text, and shutdown");
    return 0;
}
