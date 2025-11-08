#pragma once
/* Fixed-width, pointer-free METHOD_BUFFERED ABI. Keep service/protocol.rs in sync. */
#define BL_VERSION 1u
#define BL_MAX_RULES 32u
#define BL_MAX_FLOWS 512u
#define BL_DEVICE_TYPE 0x22u
#define BL_IOCTL(n) ((BL_DEVICE_TYPE << 16) | (3u << 14) | ((0x800u + (n)) << 2))
#define BL_CONFIGURE BL_IOCTL(0)
#define BL_HEARTBEAT BL_IOCTL(1)
#define BL_DRAIN BL_IOCTL(2)
#define BL_QUERY BL_IOCTL(3)
#define BL_CLEAR_FLOWS BL_IOCTL(4)
#define BL_QUIESCE BL_IOCTL(5)
#define BL_STATE_DRAINING 1u
#define BL_STATE_ARMED 2u
#define BL_STATE_QUIESCED 3u
#define BL_TICKS_PER_SECOND 10000000ull
#define BL_MAX_RATE (1024ull * 1024ull * 1024ull)

typedef unsigned long long BL_U64;
typedef unsigned int BL_U32;
typedef struct BL_CONFIG {
    BL_U32 version, size, count, lease_ms;
    BL_U64 download_bytes_per_second[BL_MAX_RULES];
} BL_CONFIG;
typedef struct BL_STATUS {
    BL_U32 version, size, state, pending;
    BL_U32 flows, filters, continuations, reserved;
    BL_U64 admitted_bytes, deferrals, continue_failures, untracked_flows;
} BL_STATUS;

/* Stable GUID namespace: data1 below; data2=0x752d, data3=0x4ae7,
   data4={0xb3,0x5f,0x91,0x83,0x2c,0x65,0x07,0x11}. */
#define BL_PROVIDER_ID 0xb1070001u
#define BL_SUBLAYER_ID 0xb1070002u
#define BL_ALE4_ID 0xb1070010u
#define BL_ALE6_ID 0xb1070011u
#define BL_STREAM4_ID 0xb1070012u
#define BL_STREAM6_ID 0xb1070013u
#define BL_STREAM_FILTER4_ID 0xb1070020u
#define BL_STREAM_FILTER6_ID 0xb1070021u
#define BL_APP_FILTER_BASE 0xb1070100u
