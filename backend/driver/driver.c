/* Experimental inbound TCP stream shaper. WDK build and VM validation required. */
#include <ntddk.h>
#include <ndis.h>
#include <fwpsk.h>
#include <wdmsec.h>
#include "../shared/budget.h"

#define BL_TAG 'fLmB'
#define BL_GUID(n) { (n), 0x752d, 0x4ae7, {0xb3,0x5f,0x91,0x83,0x2c,0x65,0x07,0x11} }
static const GUID gKeys[4] = { BL_GUID(BL_ALE4_ID), BL_GUID(BL_ALE6_ID),
    BL_GUID(BL_STREAM4_ID), BL_GUID(BL_STREAM6_ID) };
static const GUID gDeviceClass = BL_GUID(0xb1070030u);

typedef struct BL_FLOW {
    LIST_ENTRY link;
    LONG references;
    UINT64 handle;
    UINT32 callout, flags, slot;
    UINT16 layer;
    BOOLEAN closing, deferred;
} BL_FLOW;

static KSPIN_LOCK gLock;
static KMUTEX gControlLock;
static LIST_ENTRY gFlows;
static BL_BUDGET gBudgets[BL_MAX_RULES];
static BL_STATUS gStatus;
static UINT32 gIds[4], gRuleCount;
static UINT64 gLeaseUntil, gTimerDue;
static ULONG gLeaseMs;
static BOOLEAN gScheduled;
static KTIMER gTimer;
static KDPC gDpc;
static PDEVICE_OBJECT gDevice;
static PDRIVER_OBJECT gDriver;
static UNICODE_STRING gLink;
DRIVER_UNLOAD BlUnload;

/* Must hold gLock. Idle armed operation wakes once per second for the lease. */
static VOID BlScheduleLocked(ULONG milliseconds) {
    UINT64 due = KeQueryInterruptTime() + (UINT64)milliseconds * 10000;
    if (!gScheduled || due < gTimerDue) {
        LARGE_INTEGER relative;
        relative.QuadPart = -(LONGLONG)milliseconds * 10000;
        gScheduled = TRUE; gTimerDue = due;
        KeSetTimer(&gTimer, relative, &gDpc);
    }
}
static VOID BlRelease(BL_FLOW *flow) {
    if (InterlockedDecrement(&flow->references) == 0) ExFreePoolWithTag(flow, BL_TAG);
}
static VOID BlDrainLocked(VOID) {
    if (gStatus.state != BL_STATE_ARMED && gStatus.state != BL_STATE_DRAINING) return;
    gStatus.state = BL_STATE_DRAINING;
    if (gStatus.pending != 0) BlScheduleLocked(1);
}

static VOID NTAPI BlFlowDelete(UINT16 layerId, UINT32 calloutId, UINT64 context) {
    BL_FLOW *flow = (BL_FLOW *)(ULONG_PTR)context;
    KIRQL irql;
    UNREFERENCED_PARAMETER(layerId); UNREFERENCED_PARAMETER(calloutId);
    KeAcquireSpinLock(&gLock, &irql);
    flow->closing = TRUE;
    RemoveEntryList(&flow->link);
    --gStatus.flows;
    if (flow->deferred) { flow->deferred = FALSE; --gStatus.pending; }
    KeReleaseSpinLock(&gLock, irql);
    BlRelease(flow); /* release WFP ownership; timer snapshots own separate refs */
}

static VOID NTAPI BlAleClassify(const FWPS_INCOMING_VALUES0 *values,
    const FWPS_INCOMING_METADATA_VALUES0 *metadata, VOID *data,
    const FWPS_FILTER0 *filter, UINT64 context, FWPS_CLASSIFY_OUT0 *out) {
    BL_FLOW *flow;
    UINT32 slot;
    NTSTATUS status;
    KIRQL irql;
    UNREFERENCED_PARAMETER(data); UNREFERENCED_PARAMETER(context);
    if (out->rights & FWPS_RIGHT_ACTION_WRITE) out->actionType = FWP_ACTION_CONTINUE;
    if (!(metadata->currentMetadataValues & FWPS_METADATA_FIELD_FLOW_HANDLE) ||
        filter->context == 0 || filter->context > BL_MAX_RULES) return;
    slot = (UINT32)filter->context - 1;
    flow = ExAllocatePool2(POOL_FLAG_NON_PAGED, sizeof(*flow), BL_TAG);
    if (flow == NULL) {
        KeAcquireSpinLock(&gLock, &irql); ++gStatus.untracked_flows; KeReleaseSpinLock(&gLock, irql); return;
    }
    RtlZeroMemory(flow, sizeof(*flow));
    flow->handle = metadata->flowHandle; flow->slot = slot;
    flow->layer = values->layerId == FWPS_LAYER_ALE_FLOW_ESTABLISHED_V4 ? FWPS_LAYER_STREAM_V4 : FWPS_LAYER_STREAM_V6;
    flow->callout = flow->layer == FWPS_LAYER_STREAM_V4 ? gIds[2] : gIds[3];
    /* One provisional WFP ref and one association-call ref: deletion may be synchronous. */
    flow->references = 2;
    KeAcquireSpinLock(&gLock, &irql);
    if (gStatus.state != BL_STATE_ARMED || slot >= gRuleCount || gStatus.flows >= BL_MAX_FLOWS) {
        ++gStatus.untracked_flows;
        KeReleaseSpinLock(&gLock, irql); ExFreePoolWithTag(flow, BL_TAG); return;
    }
    InsertTailList(&gFlows, &flow->link); ++gStatus.flows;
    KeReleaseSpinLock(&gLock, irql);
    status = FwpsFlowAssociateContext0(flow->handle, flow->layer, flow->callout, (UINT64)(ULONG_PTR)flow);
    if (!NT_SUCCESS(status)) {
        KeAcquireSpinLock(&gLock, &irql);
        RemoveEntryList(&flow->link); --gStatus.flows; ++gStatus.untracked_flows;
        KeReleaseSpinLock(&gLock, irql); BlRelease(flow);
    }
    BlRelease(flow);
}

static VOID NTAPI BlStreamClassify(const FWPS_INCOMING_VALUES0 *values,
    const FWPS_INCOMING_METADATA_VALUES0 *metadata, VOID *data,
    const FWPS_FILTER0 *filter, UINT64 context, FWPS_CLASSIFY_OUT0 *out) {
    FWPS_STREAM_CALLOUT_IO_PACKET0 *packet = data;
    BL_FLOW *flow = (BL_FLOW *)(ULONG_PTR)context;
    SIZE_T allowed;
    KIRQL irql;
    UNREFERENCED_PARAMETER(values); UNREFERENCED_PARAMETER(metadata); UNREFERENCED_PARAMETER(filter);
    /* Stream layers require an explicit action regardless of ACTION_WRITE rights. */
    out->actionType = FWP_ACTION_PERMIT;
    if (packet == NULL || packet->streamData == NULL) return;
    packet->countBytesRequired = 0; packet->streamAction = FWPS_STREAM_ACTION_NONE;
    packet->countBytesEnforced = packet->streamData->dataLength;
    if (flow == NULL || !(packet->streamData->flags & FWPS_STREAM_FLAG_RECEIVE) ||
        packet->streamData->dataLength == 0) return; /* SEND is never deferred */
    KeAcquireSpinLock(&gLock, &irql);
    if (gStatus.state == BL_STATE_ARMED && !flow->closing) {
        allowed = (SIZE_T)BlBudgetTake(&gBudgets[flow->slot], packet->streamData->dataLength, KeQueryInterruptTime());
        packet->countBytesEnforced = allowed;
        if (allowed == 0) {
            packet->streamAction = FWPS_STREAM_ACTION_DEFER;
            flow->flags = packet->streamData->flags;
            if (!flow->deferred) { flow->deferred = TRUE; ++gStatus.pending; }
            ++gStatus.deferrals; BlScheduleLocked(10);
        } else { gStatus.admitted_bytes += allowed; }
    }
    KeReleaseSpinLock(&gLock, irql);
}

static NTSTATUS NTAPI BlNotify(FWPS_CALLOUT_NOTIFY_TYPE type, const GUID *key, FWPS_FILTER0 *filter) {
    KIRQL irql;
    UNREFERENCED_PARAMETER(key); UNREFERENCED_PARAMETER(filter);
    KeAcquireSpinLock(&gLock, &irql);
    if (type == FWPS_CALLOUT_NOTIFY_ADD_FILTER) ++gStatus.filters;
    else if (type == FWPS_CALLOUT_NOTIFY_DELETE_FILTER && gStatus.filters != 0) --gStatus.filters;
    KeReleaseSpinLock(&gLock, irql);
    return STATUS_SUCCESS;
}

static VOID BlTimerDpc(KDPC *dpc, VOID *context, VOID *arg1, VOID *arg2) {
    struct { BL_FLOW *flow; UINT32 flags; } batch[32];
    ULONG count = 0, i;
    PLIST_ENTRY item;
    KIRQL irql;
    UINT64 now = KeQueryInterruptTime();
    UNREFERENCED_PARAMETER(dpc); UNREFERENCED_PARAMETER(context);
    UNREFERENCED_PARAMETER(arg1); UNREFERENCED_PARAMETER(arg2);
    KeAcquireSpinLock(&gLock, &irql); gScheduled = FALSE;
    if (gStatus.state == BL_STATE_ARMED && now >= gLeaseUntil) BlDrainLocked();
    for (item = gFlows.Flink; item != &gFlows && count < RTL_NUMBER_OF(batch); item = item->Flink) {
        BL_FLOW *flow = CONTAINING_RECORD(item, BL_FLOW, link);
        if (flow->deferred && !flow->closing && (gStatus.state != BL_STATE_ARMED ||
            BlBudgetAvailable(&gBudgets[flow->slot], now) != 0)) {
            flow->deferred = FALSE; --gStatus.pending; ++gStatus.continuations;
            InterlockedIncrement(&flow->references); batch[count].flow = flow;
            batch[count++].flags = flow->flags;
        }
    }
    for (i = 0; i < count; ++i) { RemoveEntryList(&batch[i].flow->link); InsertTailList(&gFlows, &batch[i].flow->link); }
    KeReleaseSpinLock(&gLock, irql);
    for (i = 0; i < count; ++i) {
        BL_FLOW *flow = batch[i].flow;
        /* Not under a spinlock and never on a classify callback's stack. */
        NTSTATUS result = FwpsStreamContinue0(flow->handle, flow->callout, flow->layer, batch[i].flags);
        KeAcquireSpinLock(&gLock, &irql); --gStatus.continuations;
        if (!NT_SUCCESS(result)) {
            ++gStatus.continue_failures;
            if (!flow->closing && !flow->deferred) { flow->deferred = TRUE; ++gStatus.pending; }
        }
        KeReleaseSpinLock(&gLock, irql); BlRelease(flow);
    }
    KeAcquireSpinLock(&gLock, &irql);
    if (gStatus.pending != 0) BlScheduleLocked(10);
    else if (gStatus.state == BL_STATE_ARMED) BlScheduleLocked(1000);
    KeReleaseSpinLock(&gLock, irql);
}

/* Called at PASSIVE_LEVEL, only after the broker has drained and removed filters. */
static NTSTATUS BlClearFlows(VOID) {
    BL_FLOW **batch;
    PLIST_ENTRY item;
    ULONG count = 0, i;
    KIRQL irql;
    batch = ExAllocatePool2(POOL_FLAG_NON_PAGED, sizeof(*batch) * BL_MAX_FLOWS, BL_TAG);
    if (batch == NULL) return STATUS_INSUFFICIENT_RESOURCES;
    KeAcquireSpinLock(&gLock, &irql);
    if ((gStatus.state != BL_STATE_DRAINING && gStatus.state != 4) || gStatus.filters || gStatus.pending || gStatus.continuations) {
        KeReleaseSpinLock(&gLock, irql); ExFreePoolWithTag(batch, BL_TAG); return STATUS_DEVICE_BUSY;
    }
    for (item = gFlows.Flink; item != &gFlows; item = item->Flink) {
        BL_FLOW *flow = CONTAINING_RECORD(item, BL_FLOW, link);
        InterlockedIncrement(&flow->references); batch[count++] = flow;
    }
    KeReleaseSpinLock(&gLock, irql);
    for (i = 0; i < count; ++i) {
        FwpsFlowRemoveContext0(batch[i]->handle, batch[i]->layer, batch[i]->callout);
        BlRelease(batch[i]);
    }
    ExFreePoolWithTag(batch, BL_TAG);
    KeAcquireSpinLock(&gLock, &irql); count = gStatus.flows; KeReleaseSpinLock(&gLock, irql);
    return count == 0 ? STATUS_SUCCESS : STATUS_DEVICE_BUSY;
}

static NTSTATUS BlComplete(IRP *irp, NTSTATUS status, ULONG_PTR bytes) {
    irp->IoStatus.Status = status; irp->IoStatus.Information = bytes;
    IoCompleteRequest(irp, IO_NO_INCREMENT); return status;
}
static NTSTATUS BlControlRequest(PDEVICE_OBJECT device, IRP *irp) {
    PIO_STACK_LOCATION stack = IoGetCurrentIrpStackLocation(irp);
    ULONG code = stack->Parameters.DeviceIoControl.IoControlCode;
    VOID *buffer = irp->AssociatedIrp.SystemBuffer;
    NTSTATUS status = STATUS_SUCCESS;
    KIRQL irql;
    UINT32 i;
    UNREFERENCED_PARAMETER(device);
    if (KeGetCurrentIrql() != PASSIVE_LEVEL) return BlComplete(irp, STATUS_INVALID_DEVICE_STATE, 0);
    if (code == BL_CLEAR_FLOWS) return BlComplete(irp, BlClearFlows(), 0);
    if (code == BL_QUIESCE) {
        status = BlClearFlows();
        if (!NT_SUCCESS(status)) return BlComplete(irp, status, 0);
        KeAcquireSpinLock(&gLock, &irql); gStatus.state = 4; /* terminal quiescing; configuration stays disabled */
        KeReleaseSpinLock(&gLock, irql);
        for (i = 0; i < 4; ++i) if (gIds[i]) {
            status = FwpsCalloutUnregisterById0(gIds[i]);
            if (!NT_SUCCESS(status)) return BlComplete(irp, status, 0);
            gIds[i] = 0;
        }
        KeAcquireSpinLock(&gLock, &irql); gStatus.state = BL_STATE_QUIESCED;
        KeReleaseSpinLock(&gLock, irql);
        gDriver->DriverUnload = BlUnload;
        return BlComplete(irp, STATUS_SUCCESS, 0);
    }
    KeAcquireSpinLock(&gLock, &irql);
    switch (code) {
    case BL_CONFIGURE: {
        const BL_CONFIG *config = buffer;
        if (stack->Parameters.DeviceIoControl.InputBufferLength != sizeof(BL_CONFIG) || !BlConfigValid(config)) {
            status = STATUS_INVALID_PARAMETER; break;
        }
        if (gStatus.state != BL_STATE_DRAINING || gStatus.flows || gStatus.filters || gStatus.continuations) {
            status = STATUS_DEVICE_BUSY; break;
        }
        gRuleCount = config->count; gLeaseMs = config->lease_ms;
        for (i = 0; i < gRuleCount; ++i) BlBudgetInit(&gBudgets[i], config->download_bytes_per_second[i], KeQueryInterruptTime());
        gLeaseUntil = KeQueryInterruptTime() + (UINT64)gLeaseMs * 10000;
        gStatus.state = BL_STATE_ARMED; BlScheduleLocked(1000); break;
    }
    case BL_HEARTBEAT:
        if (gStatus.state != BL_STATE_ARMED) status = STATUS_INVALID_DEVICE_STATE;
        else gLeaseUntil = KeQueryInterruptTime() + (UINT64)gLeaseMs * 10000;
        break;
    case BL_DRAIN:
        if (gStatus.state != BL_STATE_QUIESCED) BlDrainLocked();
        break;
    case BL_QUERY:
        if (stack->Parameters.DeviceIoControl.OutputBufferLength < sizeof(BL_STATUS)) status = STATUS_BUFFER_TOO_SMALL;
        else { RtlCopyMemory(buffer, &gStatus, sizeof(gStatus)); }
        break;
    default: status = STATUS_INVALID_DEVICE_REQUEST; break;
    }
    KeReleaseSpinLock(&gLock, irql);
    return BlComplete(irp, status, NT_SUCCESS(status) && code == BL_QUERY ? sizeof(BL_STATUS) : 0);
}
static NTSTATUS BlDeviceControl(PDEVICE_OBJECT device, IRP *irp) {
    NTSTATUS status;
    if (KeGetCurrentIrql() != PASSIVE_LEVEL) return BlComplete(irp, STATUS_INVALID_DEVICE_STATE, 0);
    KeWaitForSingleObject(&gControlLock, Executive, KernelMode, FALSE, NULL);
    status = BlControlRequest(device, irp);
    KeReleaseMutex(&gControlLock, FALSE);
    return status;
}
static NTSTATUS BlOpenClose(PDEVICE_OBJECT device, IRP *irp) {
    KIRQL irql;
    UNREFERENCED_PARAMETER(device);
    if (IoGetCurrentIrpStackLocation(irp)->MajorFunction == IRP_MJ_CLEANUP) {
        KeAcquireSpinLock(&gLock, &irql);
        if (gStatus.state != BL_STATE_QUIESCED) BlDrainLocked();
        KeReleaseSpinLock(&gLock, irql);
    }
    return BlComplete(irp, STATUS_SUCCESS, 0);
}
VOID BlUnload(PDRIVER_OBJECT driver) {
    UNREFERENCED_PARAMETER(driver);
    KeCancelTimer(&gTimer); KeFlushQueuedDpcs();
    IoDeleteSymbolicLink(&gLink); IoDeleteDevice(gDevice);
}

DRIVER_INITIALIZE DriverEntry;
NTSTATUS DriverEntry(PDRIVER_OBJECT driver, PUNICODE_STRING registry) {
    UNICODE_STRING name = RTL_CONSTANT_STRING(L"\\Device\\BandwidthLimiterWfp");
    UNICODE_STRING sddl = RTL_CONSTANT_STRING(L"D:P(A;;GA;;;SY)(A;;GA;;;BA)");
    NTSTATUS status;
    UINT32 i;
    UNREFERENCED_PARAMETER(registry);
    C_ASSERT(sizeof(BL_CONFIG) == 272); C_ASSERT(sizeof(BL_STATUS) == 64);
    gDriver = driver; KeInitializeSpinLock(&gLock); InitializeListHead(&gFlows);
    KeInitializeMutex(&gControlLock, 0);
    KeInitializeTimer(&gTimer); KeInitializeDpc(&gDpc, BlTimerDpc, NULL);
    gStatus.version = BL_VERSION; gStatus.size = sizeof(gStatus); gStatus.state = BL_STATE_DRAINING;
    RtlInitUnicodeString(&gLink, L"\\DosDevices\\BandwidthLimiterWfp");
    status = IoCreateDeviceSecure(driver, 0, &name, FILE_DEVICE_UNKNOWN,
        FILE_DEVICE_SECURE_OPEN, TRUE, &sddl, &gDeviceClass, &gDevice);
    if (!NT_SUCCESS(status)) return status;
    for (i = 0; i < 4; ++i) {
        FWPS_CALLOUT0 callout = {0};
        callout.calloutKey = gKeys[i]; callout.notifyFn = BlNotify;
        callout.classifyFn = i < 2 ? BlAleClassify : BlStreamClassify;
        if (i >= 2) { callout.flowDeleteFn = BlFlowDelete; callout.flags = FWP_CALLOUT_FLAG_CONDITIONAL_ON_FLOW; }
        status = FwpsCalloutRegister0(gDevice, &callout, &gIds[i]);
        if (!NT_SUCCESS(status)) goto failed;
    }
    status = IoCreateSymbolicLink(&gLink, &name);
    if (!NT_SUCCESS(status)) goto failed;
    driver->MajorFunction[IRP_MJ_CREATE] = BlOpenClose;
    driver->MajorFunction[IRP_MJ_CLOSE] = BlOpenClose;
    driver->MajorFunction[IRP_MJ_CLEANUP] = BlOpenClose;
    driver->MajorFunction[IRP_MJ_DEVICE_CONTROL] = BlDeviceControl;
    /* Unload is exposed only by successful explicit QUIESCE after filter/flow drain. */
    driver->DriverUnload = NULL;
    gDevice->Flags &= ~DO_DEVICE_INITIALIZING;
    return STATUS_SUCCESS;
failed:
    for (i = 0; i < 4; ++i) if (gIds[i]) { FwpsCalloutUnregisterById0(gIds[i]); gIds[i] = 0; }
    IoDeleteDevice(gDevice); return status;
}
