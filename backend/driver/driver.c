/* Experimental bidirectional TCP OOB shaper. Requires WDK and isolated VM verification. */
#include <ntddk.h>
#include <ndis.h>
#include <fwpsk.h>
#include <wdmsec.h>
#include "../shared/budget.h"
#define BL_TAG 'fLmB'
#define BL_GUID(n) { (n), 0x752d, 0x4ae7, {0xb3,0x5f,0x91,0x83,0x2c,0x65,0x07,0x11} }
static const GUID gKeys[4] = { BL_GUID(BL_ALE4_ID), BL_GUID(BL_ALE6_ID), BL_GUID(BL_STREAM4_ID), BL_GUID(BL_STREAM6_ID) };
static const GUID gDeviceClass = BL_GUID(0xb1070030u);
typedef struct BL_FLOW {
    LIST_ENTRY link, queue[2]; LONG references;
    UINT64 handle; UINT32 callout, slot, items; UINT16 layer;
    BOOLEAN closing, failed, abortIssued, busy[2];
} BL_FLOW;
typedef struct BL_ITEM {
    LIST_ENTRY link; BL_FLOW *flow; NET_BUFFER_LIST *chain;
    SIZE_T bytes; UINT32 flags, direction; LONG references, failed;
} BL_ITEM;
static KSPIN_LOCK gLock;
static KMUTEX gControl;
static LIST_ENTRY gFlows;
static BL_BUDGET gBudgets[BL_MAX_RULES][2];
static BL_STATUS gStatus;
static UINT32 gIds[4], gRules; static UINT64 gLeaseUntil, gTimerDue; static ULONG gLeaseMs;
static BOOLEAN gScheduled; static KTIMER gTimer; static KDPC gDpc;
static HANDLE gInjection; static PDEVICE_OBJECT gDevice; static PDRIVER_OBJECT gDriver;
static UNICODE_STRING gLink;
DRIVER_UNLOAD BlUnload;
static VOID BlScheduleLocked(ULONG ms) {
    UINT64 due = KeQueryInterruptTime() + (UINT64)ms * 10000;
    if (!gScheduled || due < gTimerDue) { LARGE_INTEGER relative; relative.QuadPart = -(LONGLONG)ms * 10000;
        gScheduled = TRUE; gTimerDue = due; KeSetTimer(&gTimer, relative, &gDpc); }
}
static VOID BlFlowRelease(BL_FLOW *flow) { if (InterlockedDecrement(&flow->references) == 0) ExFreePoolWithTag(flow, BL_TAG); }
static VOID BlDrainLocked(VOID) {
    if (gStatus.state == BL_STATE_ARMED || gStatus.state == BL_STATE_DRAINING) {
        gStatus.state = BL_STATE_DRAINING; if (gStatus.flows) BlScheduleLocked(1);
    }
}
static VOID BlItemDiscard(BL_ITEM *item) {
    KIRQL irql;
    if (item->chain) FwpsDiscardClonedStreamData0(item->chain, 0, KeGetCurrentIrql() == DISPATCH_LEVEL);
    KeAcquireSpinLock(&gLock, &irql); --gStatus.pending; gStatus.queued_bytes -= item->bytes;
    --item->flow->items; KeReleaseSpinLock(&gLock, irql);
    BlFlowRelease(item->flow); ExFreePoolWithTag(item, BL_TAG);
}
static VOID BlInjectionRelease(BL_ITEM *item) {
    KIRQL irql;
    if (InterlockedDecrement(&item->references) != 0) return;
    KeAcquireSpinLock(&gLock, &irql);
    --gStatus.pending; --gStatus.continuations; gStatus.queued_bytes -= item->bytes;
    --item->flow->items; item->flow->busy[item->direction] = FALSE;
    if (item->failed) { ++gStatus.inject_failures; item->flow->failed = TRUE; }
    BlScheduleLocked(1);
    KeReleaseSpinLock(&gLock, irql);
    BlFlowRelease(item->flow); ExFreePoolWithTag(item, BL_TAG);
}
static VOID NTAPI BlInjected(VOID *context, NET_BUFFER_LIST *nbl, BOOLEAN dispatch) {
    BL_ITEM *item = context;
    UNREFERENCED_PARAMETER(dispatch);
    if (!NT_SUCCESS(NET_BUFFER_LIST_STATUS(nbl))) InterlockedExchange(&item->failed, TRUE);
    FwpsFreeCloneNetBufferList0(nbl, 0); /* API defines no free-clone flags at either IRQL */
    BlInjectionRelease(item);
}
static VOID NTAPI BlFlowDelete(UINT16 layer, UINT32 callout, UINT64 context) {
    BL_FLOW *flow = (BL_FLOW *)(ULONG_PTR)context; LIST_ENTRY discarded; KIRQL irql; UINT32 d;
    UNREFERENCED_PARAMETER(layer); UNREFERENCED_PARAMETER(callout); InitializeListHead(&discarded);
    KeAcquireSpinLock(&gLock, &irql); flow->closing = TRUE; RemoveEntryList(&flow->link); --gStatus.flows;
    for (d = 0; d < 2; ++d) while (!IsListEmpty(&flow->queue[d])) InsertTailList(&discarded, RemoveHeadList(&flow->queue[d]));
    KeReleaseSpinLock(&gLock, irql);
    while (!IsListEmpty(&discarded)) BlItemDiscard(CONTAINING_RECORD(RemoveHeadList(&discarded), BL_ITEM, link));
    BlFlowRelease(flow); /* in-flight items retain separate references */
}
static VOID NTAPI BlAle(const FWPS_INCOMING_VALUES0 *values, const FWPS_INCOMING_METADATA_VALUES0 *meta,
    VOID *data, const FWPS_FILTER0 *filter, UINT64 context, FWPS_CLASSIFY_OUT0 *out) {
    BL_FLOW *flow; NTSTATUS status; KIRQL irql; UINT32 slot;
    UNREFERENCED_PARAMETER(data); UNREFERENCED_PARAMETER(context);
    if (out->rights & FWPS_RIGHT_ACTION_WRITE) out->actionType = FWP_ACTION_CONTINUE;
    if (!(meta->currentMetadataValues & FWPS_METADATA_FIELD_FLOW_HANDLE) || filter->context == 0 || filter->context > BL_MAX_RULES) return;
    slot = (UINT32)filter->context - 1;
    flow = ExAllocatePool2(POOL_FLAG_NON_PAGED, sizeof(*flow), BL_TAG);
    if (!flow) { KeAcquireSpinLock(&gLock, &irql); ++gStatus.untracked_flows; KeReleaseSpinLock(&gLock, irql); return; }
    RtlZeroMemory(flow, sizeof(*flow)); InitializeListHead(&flow->queue[0]); InitializeListHead(&flow->queue[1]);
    flow->handle = meta->flowHandle; flow->slot = slot; flow->references = 2;
    flow->layer = (UINT16)(values->layerId == FWPS_LAYER_ALE_FLOW_ESTABLISHED_V4 ? FWPS_LAYER_STREAM_V4 : FWPS_LAYER_STREAM_V6);
    flow->callout = flow->layer == FWPS_LAYER_STREAM_V4 ? gIds[2] : gIds[3];
    KeAcquireSpinLock(&gLock, &irql);
    if (gStatus.state != BL_STATE_ARMED || slot >= gRules || gStatus.flows >= BL_MAX_FLOWS) {
        ++gStatus.untracked_flows; KeReleaseSpinLock(&gLock, irql); ExFreePoolWithTag(flow, BL_TAG); return;
    }
    InsertTailList(&gFlows, &flow->link); ++gStatus.flows; KeReleaseSpinLock(&gLock, irql);
    status = FwpsFlowAssociateContext0(flow->handle, flow->layer, flow->callout, (UINT64)(ULONG_PTR)flow);
    if (!NT_SUCCESS(status)) {
        KeAcquireSpinLock(&gLock, &irql); RemoveEntryList(&flow->link); --gStatus.flows; ++gStatus.untracked_flows;
        KeReleaseSpinLock(&gLock, irql); BlFlowRelease(flow);
    }
    BlFlowRelease(flow);
}
static VOID NTAPI BlStream(const FWPS_INCOMING_VALUES0 *values, const FWPS_INCOMING_METADATA_VALUES0 *meta,
    VOID *data, const FWPS_FILTER0 *filter, UINT64 context, FWPS_CLASSIFY_OUT0 *out) {
    FWPS_STREAM_CALLOUT_IO_PACKET0 *packet = data; BL_FLOW *flow = (BL_FLOW *)(ULONG_PTR)context;
    BL_ITEM *item; SIZE_T bytes; UINT32 flags, direction; KIRQL irql; NTSTATUS status = STATUS_SUCCESS;
    UNREFERENCED_PARAMETER(values); UNREFERENCED_PARAMETER(meta); UNREFERENCED_PARAMETER(filter);
    out->actionType = FWP_ACTION_PERMIT;
    if (!packet || !packet->streamData || !flow) return;
    packet->countBytesRequired = 0; packet->countBytesEnforced = packet->streamData->dataLength;
    packet->streamAction = FWPS_STREAM_ACTION_NONE;
    bytes = packet->streamData->dataLength; flags = packet->streamData->flags;
    direction = (flags & FWPS_STREAM_FLAG_SEND) ? 1u : 0u;
    if (bytes == 0 && !(flags & (FWPS_STREAM_FLAG_SEND_DISCONNECT | FWPS_STREAM_FLAG_RECEIVE_DISCONNECT | FWPS_STREAM_FLAG_SEND_ABORT | FWPS_STREAM_FLAG_RECEIVE_ABORT))) return;
    out->actionType = FWP_ACTION_BLOCK; /* clone/absorb ALL selected payload in BOTH directions */
    KeAcquireSpinLock(&gLock, &irql);
    if (flow->closing || flow->failed || gStatus.state != BL_STATE_ARMED || bytes > BL_MAX_SEGMENT ||
        gStatus.pending >= BL_MAX_SEGMENTS || flow->items >= BL_MAX_FLOW_SEGMENTS ||
        bytes > BL_MAX_QUEUED_BYTES - gStatus.queued_bytes || (flags & (FWPS_STREAM_FLAG_SEND_ABORT | FWPS_STREAM_FLAG_RECEIVE_ABORT))) {
        flow->failed = TRUE; ++gStatus.inject_failures; BlScheduleLocked(1);
        KeReleaseSpinLock(&gLock, irql); packet->streamAction = FWPS_STREAM_ACTION_DROP_CONNECTION; return;
    }
    ++gStatus.pending; ++flow->items; gStatus.queued_bytes += bytes;
    if (gStatus.queued_bytes > gStatus.queued_peak) gStatus.queued_peak = gStatus.queued_bytes;
    InterlockedIncrement(&flow->references); KeReleaseSpinLock(&gLock, irql);
    item = ExAllocatePool2(POOL_FLAG_NON_PAGED, sizeof(*item), BL_TAG);
    if (item) {
        RtlZeroMemory(item, sizeof(*item)); item->flow = flow; item->bytes = bytes; item->flags = flags; item->direction = direction;
        if (bytes != 0) status = FwpsCloneStreamData0(packet->streamData, NULL, NULL, 0, &item->chain);
        if (NT_SUCCESS(status) && item->chain) {
            NET_BUFFER_LIST *node; ULONG nodes = 0;
            for (node = item->chain; node; node = NET_BUFFER_LIST_NEXT_NBL(node)) if (++nodes > 256) { status = STATUS_INSUFFICIENT_RESOURCES; break; }
        }
    } else status = STATUS_INSUFFICIENT_RESOURCES;
    KeAcquireSpinLock(&gLock, &irql);
    if (!item || !NT_SUCCESS(status) || flow->closing) {
        flow->failed = TRUE; ++gStatus.inject_failures; BlScheduleLocked(1);
        KeReleaseSpinLock(&gLock, irql);
        if (item) BlItemDiscard(item);
        else { KeAcquireSpinLock(&gLock, &irql); --gStatus.pending; --flow->items; gStatus.queued_bytes -= bytes;
            KeReleaseSpinLock(&gLock, irql); BlFlowRelease(flow); }
        packet->streamAction = FWPS_STREAM_ACTION_DROP_CONNECTION; return;
    }
    InsertTailList(&flow->queue[direction], &item->link); BlScheduleLocked(10); KeReleaseSpinLock(&gLock, irql);
}
static NTSTATUS NTAPI BlNotify(FWPS_CALLOUT_NOTIFY_TYPE type, const GUID *key, FWPS_FILTER0 *filter) {
    KIRQL irql; UNREFERENCED_PARAMETER(key); UNREFERENCED_PARAMETER(filter);
    KeAcquireSpinLock(&gLock, &irql);
    if (type == FWPS_CALLOUT_NOTIFY_ADD_FILTER) ++gStatus.filters;
    else if (type == FWPS_CALLOUT_NOTIFY_DELETE_FILTER && gStatus.filters) --gStatus.filters;
    KeReleaseSpinLock(&gLock, irql); return STATUS_SUCCESS;
}
static VOID BlTimer(KDPC *dpc, VOID *ctx, VOID *a, VOID *b) {
    BL_ITEM *items[32]; BL_FLOW *aborts[32]; ULONG count = 0, abortCount = 0, i, d;
    PLIST_ENTRY link; KIRQL irql; UINT64 now = KeQueryInterruptTime();
    UNREFERENCED_PARAMETER(dpc); UNREFERENCED_PARAMETER(ctx); UNREFERENCED_PARAMETER(a); UNREFERENCED_PARAMETER(b);
    KeAcquireSpinLock(&gLock, &irql); gScheduled = FALSE;
    if (gStatus.state == BL_STATE_ARMED && now >= gLeaseUntil) BlDrainLocked();
    for (link = gFlows.Flink; link != &gFlows; link = link->Flink) {
        BL_FLOW *flow = CONTAINING_RECORD(link, BL_FLOW, link);
        if ((flow->failed || gStatus.state == BL_STATE_DRAINING) && !flow->abortIssued && abortCount < RTL_NUMBER_OF(aborts)) {
            flow->abortIssued = TRUE; InterlockedIncrement(&flow->references); aborts[abortCount++] = flow; continue;
        }
        if (flow->failed || gStatus.state != BL_STATE_ARMED) continue;
        for (d = 0; d < 2 && count < RTL_NUMBER_OF(items); ++d) if (!flow->busy[d] && !IsListEmpty(&flow->queue[d])) {
            BL_ITEM *item = CONTAINING_RECORD(flow->queue[d].Flink, BL_ITEM, link);
            if (BlBudgetTakeWhole(&gBudgets[flow->slot][d], item->bytes, now)) {
                RemoveEntryList(&item->link); flow->busy[d] = TRUE; ++gStatus.continuations;
                items[count++] = item;
            }
        }
    }
    /* Rotate selected flows to avoid a fixed head-of-list preference. */
    for (i = 0; i < count; ++i) { RemoveEntryList(&items[i]->flow->link); InsertTailList(&gFlows, &items[i]->flow->link); }
    KeReleaseSpinLock(&gLock, irql);
    for (i = 0; i < abortCount; ++i) {
        NTSTATUS result = FwpsFlowAbort0(aborts[i]->handle);
        KeAcquireSpinLock(&gLock, &irql);
        if (NT_SUCCESS(result)) ++gStatus.aborted_flows;
        else { ++gStatus.inject_failures; if (!aborts[i]->closing) aborts[i]->abortIssued = FALSE; }
        KeReleaseSpinLock(&gLock, irql); BlFlowRelease(aborts[i]);
    }
    for (i = 0; i < count; ++i) {
        BL_ITEM *item = items[i]; NET_BUFFER_LIST *nbl; LONG callbacks = 0; NTSTATUS result;
        for (nbl = item->chain; nbl; nbl = NET_BUFFER_LIST_NEXT_NBL(nbl)) ++callbacks;
        item->references = callbacks + 1; /* setup ref survives synchronous completion callbacks */
        result = FwpsStreamInjectAsync0(gInjection, NULL, 0, item->flow->handle, item->flow->callout,
            item->flow->layer, item->flags, item->chain, item->bytes, BlInjected, item);
        if (!NT_SUCCESS(result)) {
            if (item->chain) FwpsDiscardClonedStreamData0(item->chain, 0, TRUE);
            item->references = 1; item->failed = TRUE; /* failed initiation has no callbacks */
        } else {
            KeAcquireSpinLock(&gLock, &irql); ++gStatus.injections;
            if (item->direction) gStatus.upload_bytes += item->bytes; else gStatus.admitted_bytes += item->bytes;
            KeReleaseSpinLock(&gLock, irql);
        }
        BlInjectionRelease(item); /* NULL FIN chains also finish here: WFP has no callback for them */
    }
    KeAcquireSpinLock(&gLock, &irql);
    if (gStatus.pending || (gStatus.state == BL_STATE_DRAINING && gStatus.flows)) BlScheduleLocked(10);
    else if (gStatus.state == BL_STATE_ARMED) BlScheduleLocked(1000);
    KeReleaseSpinLock(&gLock, irql);
}
static NTSTATUS BlComplete(IRP *irp, NTSTATUS status, ULONG_PTR bytes) {
    irp->IoStatus.Status = status; irp->IoStatus.Information = bytes; IoCompleteRequest(irp, IO_NO_INCREMENT); return status;
}
static NTSTATUS BlRequest(PDEVICE_OBJECT device, IRP *irp) {
    PIO_STACK_LOCATION s = IoGetCurrentIrpStackLocation(irp); ULONG code = s->Parameters.DeviceIoControl.IoControlCode;
    VOID *buffer = irp->AssociatedIrp.SystemBuffer; NTSTATUS status = STATUS_SUCCESS; KIRQL irql; UINT32 i;
    UNREFERENCED_PARAMETER(device);
    if (code == BL_QUIESCE) {
        KeAcquireSpinLock(&gLock, &irql);
        if (gStatus.flows || gStatus.pending || gStatus.continuations || gStatus.filters || gStatus.state == BL_STATE_ARMED) status = STATUS_DEVICE_BUSY;
        else gStatus.state = BL_STATE_QUIESCING;
        KeReleaseSpinLock(&gLock, irql); if (!NT_SUCCESS(status)) return BlComplete(irp, status, 0);
        for (i = 0; i < 4; ++i) if (gIds[i]) { status = FwpsCalloutUnregisterById0(gIds[i]); if (!NT_SUCCESS(status)) return BlComplete(irp, status, 0); gIds[i] = 0; }
        if (gInjection) { status = FwpsInjectionHandleDestroy0(gInjection); if (!NT_SUCCESS(status)) return BlComplete(irp, status, 0); gInjection = NULL; }
        KeAcquireSpinLock(&gLock, &irql); gStatus.state = BL_STATE_QUIESCED; KeReleaseSpinLock(&gLock, irql);
        gDriver->DriverUnload = BlUnload; return BlComplete(irp, STATUS_SUCCESS, 0);
    }
    KeAcquireSpinLock(&gLock, &irql);
    switch (code) {
    case BL_CONFIGURE: {
        const BL_CONFIG *c = buffer;
        if (s->Parameters.DeviceIoControl.InputBufferLength != sizeof(*c) || !BlConfigValid(c)) { status = STATUS_INVALID_PARAMETER; break; }
        if (gStatus.state != BL_STATE_DRAINING || gStatus.flows || gStatus.pending || gStatus.filters || gStatus.continuations) { status = STATUS_DEVICE_BUSY; break; }
        gRules = c->count; gLeaseMs = c->lease_ms;
        for (i = 0; i < gRules; ++i) { BlBudgetInit(&gBudgets[i][0], c->download_bytes_per_second[i], KeQueryInterruptTime()); BlBudgetInit(&gBudgets[i][1], c->upload_bytes_per_second[i], KeQueryInterruptTime()); }
        gLeaseUntil = KeQueryInterruptTime() + (UINT64)gLeaseMs * 10000; gStatus.state = BL_STATE_ARMED; BlScheduleLocked(1000); break;
    }
    case BL_HEARTBEAT:
        if (gStatus.state != BL_STATE_ARMED) status = STATUS_INVALID_DEVICE_STATE;
        else gLeaseUntil = KeQueryInterruptTime() + (UINT64)gLeaseMs * 10000; break;
    case BL_DRAIN: BlDrainLocked(); break;
    case BL_CLEAR_FLOWS:
        if (gStatus.state == BL_STATE_ARMED || gStatus.flows || gStatus.pending || gStatus.continuations || gStatus.filters) status = STATUS_DEVICE_BUSY;
        break;
    case BL_QUERY:
        if (s->Parameters.DeviceIoControl.OutputBufferLength < sizeof(gStatus)) status = STATUS_BUFFER_TOO_SMALL;
        else RtlCopyMemory(buffer, &gStatus, sizeof(gStatus)); break;
    default: status = STATUS_INVALID_DEVICE_REQUEST; break;
    }
    KeReleaseSpinLock(&gLock, irql); return BlComplete(irp, status, NT_SUCCESS(status) && code == BL_QUERY ? sizeof(gStatus) : 0);
}
static NTSTATUS BlControl(PDEVICE_OBJECT device, IRP *irp) {
    NTSTATUS status; if (KeGetCurrentIrql() != PASSIVE_LEVEL) return BlComplete(irp, STATUS_INVALID_DEVICE_STATE, 0);
    KeWaitForSingleObject(&gControl, Executive, KernelMode, FALSE, NULL); status = BlRequest(device, irp); KeReleaseMutex(&gControl, FALSE); return status;
}
static NTSTATUS BlOpenClose(PDEVICE_OBJECT device, IRP *irp) {
    KIRQL irql; UNREFERENCED_PARAMETER(device);
    if (IoGetCurrentIrpStackLocation(irp)->MajorFunction == IRP_MJ_CLEANUP) { KeAcquireSpinLock(&gLock, &irql); BlDrainLocked(); KeReleaseSpinLock(&gLock, irql); }
    return BlComplete(irp, STATUS_SUCCESS, 0);
}
VOID BlUnload(PDRIVER_OBJECT driver) { UNREFERENCED_PARAMETER(driver); KeCancelTimer(&gTimer); KeFlushQueuedDpcs(); IoDeleteSymbolicLink(&gLink); IoDeleteDevice(gDevice); }
DRIVER_INITIALIZE DriverEntry;
NTSTATUS DriverEntry(PDRIVER_OBJECT driver, PUNICODE_STRING registry) {
    UNICODE_STRING name = RTL_CONSTANT_STRING(L"\\Device\\BandwidthLimiterWfp");
    UNICODE_STRING sddl = RTL_CONSTANT_STRING(L"D:P(A;;GA;;;SY)(A;;GA;;;BA)"); NTSTATUS status; UINT32 i;
    UNREFERENCED_PARAMETER(registry); C_ASSERT(sizeof(BL_CONFIG) == 528); C_ASSERT(sizeof(BL_STATUS) == 96);
    gDriver = driver; KeInitializeSpinLock(&gLock); KeInitializeMutex(&gControl, 0); InitializeListHead(&gFlows);
    KeInitializeTimer(&gTimer); KeInitializeDpc(&gDpc, BlTimer, NULL);
    gStatus.version = BL_VERSION; gStatus.size = sizeof(gStatus); gStatus.state = BL_STATE_DRAINING;
    status = FwpsInjectionHandleCreate0(AF_UNSPEC, FWPS_INJECTION_TYPE_STREAM, &gInjection); if (!NT_SUCCESS(status)) return status;
    status = IoCreateDeviceSecure(driver, 0, &name, FILE_DEVICE_UNKNOWN, FILE_DEVICE_SECURE_OPEN, TRUE, &sddl, &gDeviceClass, &gDevice);
    if (!NT_SUCCESS(status)) { FwpsInjectionHandleDestroy0(gInjection); return status; }
    for (i = 0; i < 4; ++i) { FWPS_CALLOUT0 c = {0}; c.calloutKey = gKeys[i]; c.notifyFn = BlNotify; c.classifyFn = i < 2 ? BlAle : BlStream;
        if (i >= 2) { c.flowDeleteFn = BlFlowDelete; c.flags = FWP_CALLOUT_FLAG_CONDITIONAL_ON_FLOW; }
        status = FwpsCalloutRegister0(gDevice, &c, &gIds[i]); if (!NT_SUCCESS(status)) goto failed; }
    RtlInitUnicodeString(&gLink, L"\\DosDevices\\BandwidthLimiterWfp"); status = IoCreateSymbolicLink(&gLink, &name); if (!NT_SUCCESS(status)) goto failed;
    driver->MajorFunction[IRP_MJ_CREATE] = BlOpenClose; driver->MajorFunction[IRP_MJ_CLOSE] = BlOpenClose;
    driver->MajorFunction[IRP_MJ_CLEANUP] = BlOpenClose; driver->MajorFunction[IRP_MJ_DEVICE_CONTROL] = BlControl;
    driver->DriverUnload = NULL; gDevice->Flags &= ~DO_DEVICE_INITIALIZING; return STATUS_SUCCESS;
failed:
    for (i = 0; i < 4; ++i) if (gIds[i]) FwpsCalloutUnregisterById0(gIds[i]);
    IoDeleteDevice(gDevice); FwpsInjectionHandleDestroy0(gInjection); return status;
}
