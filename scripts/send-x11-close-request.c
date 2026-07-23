#include <X11/Xlib.h>

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>

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

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: send-x11-close-request <window-id>\n");
        return 2;
    }

    Window window = None;
    if (!parse_window(argv[1], &window)) {
        fprintf(stderr, "invalid X11 window ID: %s\n", argv[1]);
        return 2;
    }

    Display *display = XOpenDisplay(NULL);
    if (display == NULL) {
        fprintf(stderr, "could not open the X11 display\n");
        return 1;
    }

    XWindowAttributes attributes;
    if (!XGetWindowAttributes(display, window, &attributes)) {
        fprintf(stderr, "could not resolve X11 window attributes\n");
        XCloseDisplay(display);
        return 1;
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
    XFlush(display);
    XCloseDisplay(display);
    if (!sent) {
        fprintf(stderr, "could not send the X11 close request\n");
        return 1;
    }
    return 0;
}
