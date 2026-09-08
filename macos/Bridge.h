#pragma once
#include "okp_core.h"

// Keep C enum storage details at the native bridge, not in Swift UI code.
static inline bool okp_mac_result_ok(OkpLiveResult result) {
    return result == OkpLiveResult_Ok;
}
static inline bool okp_mac_is_paused(OkpPlaybackStatus status) {
    return status == OkpPlaybackStatus_Paused;
}
