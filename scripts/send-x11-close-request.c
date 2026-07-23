#include <X11/Xlib.h>

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum {
    EXIT_OPERATIONAL_ERROR = 1,
    EXIT_USAGE_ERROR = 2,
    EXIT_WINDOW_GONE = 3,
};

static int x11_error_code = Success;

static int record_x11_error(Display *display, XErrorEvent *event) {
    (void)display;
    x11_error_code = event->error_code;
    return 0;
}

static int parse_window(const char *value, Window *window) {
    char *end = NULL;
    errno = 0;
    unsigned long parsed = strtoul(value, &end, 0);
    if (errno != 0 || end == value || *end != '\0' || parsed == 0) {
        return 0;
    }
    *window = (Window)parsed;
    return 1;
}

static int read_window_attributes(Display *display, Window window,
                                  XWindowAttributes *attributes) {
    x11_error_code = Success;
    Status resolved = XGetWindowAttributes(display, window, attributes);
    XSync(display, False);
    if (x11_error_code == BadWindow) {
        return EXIT_WINDOW_GONE;
    }
    if (x11_error_code != Success || !resolved) {
        fprintf(stderr, "could not resolve X11 window attributes\n");
        return EXIT_OPERATIONAL_ERROR;
    }
    return 0;
}

int main(int argc, char **argv) {
    int probe_only = 0;
    const char *window_value = NULL;
    if (argc == 2) {
        window_value = argv[1];
    } else if (argc == 3 && strcmp(argv[1], "--probe") == 0) {
        probe_only = 1;
        window_value = argv[2];
    } else {
        fprintf(stderr,
                "usage: send-x11-close-request [--probe] <window-id>\n");
        return EXIT_USAGE_ERROR;
    }

    Window window = None;
    if (!parse_window(window_value, &window)) {
        fprintf(stderr, "invalid X11 window ID: %s\n", window_value);
        return EXIT_USAGE_ERROR;
    }

    Display *display = XOpenDisplay(NULL);
    if (display == NULL) {
        fprintf(stderr, "could not open the X11 display\n");
        return EXIT_OPERATIONAL_ERROR;
    }
    XSetErrorHandler(record_x11_error);

    XWindowAttributes attributes;
    int resolve_status = read_window_attributes(display, window, &attributes);
    if (resolve_status != 0) {
        XCloseDisplay(display);
        return resolve_status;
    }
    if (probe_only) {
        XCloseDisplay(display);
        return 0;
    }

    XEvent event = {0};
    event.xclient.type = ClientMessage;
    event.xclient.send_event = True;
    event.xclient.display = display;
    event.xclient.window = window;
    event.xclient.message_type = XInternAtom(display, "_NET_CLOSE_WINDOW", False);
    event.xclient.format = 32;
    event.xclient.data.l[0] = CurrentTime;
    event.xclient.data.l[1] = 2;

    Window root = RootWindowOfScreen(attributes.screen);
    Status sent = XSendEvent(display, root, False,
                             SubstructureRedirectMask | SubstructureNotifyMask,
                             &event);
    XSync(display, False);
    int send_error = x11_error_code;
    XCloseDisplay(display);
    if (send_error != Success || !sent) {
        fprintf(stderr, "could not send the X11 close request\n");
        return EXIT_OPERATIONAL_ERROR;
    }
    return 0;
}
