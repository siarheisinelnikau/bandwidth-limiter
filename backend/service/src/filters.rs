use crate::{policy::Policy, protocol as p};
use std::{
    ffi::c_void,
    ptr::{null, null_mut},
};
use windows_sys::{
    Win32::{Foundation::*, NetworkManagement::WindowsFilteringPlatform::*},
    core::GUID,
};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn checked(code: u32, operation: &str) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("{operation} failed: {code:#010x}"))
    }
}
fn deleted(code: u32) -> Result<(), String> {
    if code == 0
        || [
            FWP_E_FILTER_NOT_FOUND,
            FWP_E_CALLOUT_NOT_FOUND,
            FWP_E_SUBLAYER_NOT_FOUND,
            FWP_E_PROVIDER_NOT_FOUND,
        ]
        .iter()
        .any(|n| *n as u32 == code)
    {
        Ok(())
    } else {
        checked(code, "Delete owned WFP object")
    }
}
pub struct Engine(HANDLE);
impl Engine {
    pub fn open() -> Result<Self, String> {
        let mut handle = null_mut();
        // Deliberately NOT a dynamic session: crash cleanup must not remove a deferred stream filter.
        let session = FWPM_SESSION0::default();
        // SAFETY: local engine, Windows authentication, no supplied credentials; handle initialized by API.
        checked(
            unsafe { FwpmEngineOpen0(null(), 10, null(), &session, &mut handle) },
            "Open BFE engine",
        )?;
        Ok(Self(handle))
    }
    fn transaction(&self, operation: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        // SAFETY: valid engine handle; all transactions on this single owning service thread.
        unsafe {
            checked(FwpmTransactionBegin0(self.0, 0), "Begin WFP transaction")?;
            match operation() {
                Ok(()) => {
                    let code = FwpmTransactionCommit0(self.0);
                    if code != 0 {
                        FwpmTransactionAbort0(self.0);
                    }
                    checked(code, "Commit WFP transaction")
                }
                Err(e) => {
                    FwpmTransactionAbort0(self.0);
                    Err(e)
                }
            }
        }
    }
    /// Caller must first successfully drain the driver. Never call from Drop.
    pub fn remove_owned(&self) -> Result<(), String> {
        self.transaction(|| {
            // SAFETY: delete only this product's deterministic namespace; no enumeration of foreign objects.
            unsafe {
                for slot in 0..p::MAX_RULES {
                    for ipv6 in [false, true] {
                        deleted(FwpmFilterDeleteByKey0(
                            self.0,
                            &p::guid(p::app_filter(slot, ipv6)),
                        ))?;
                    }
                }
                for id in p::STREAM_FILTERS {
                    deleted(FwpmFilterDeleteByKey0(self.0, &p::guid(id)))?;
                }
                for id in p::CALLOUTS {
                    deleted(FwpmCalloutDeleteByKey0(self.0, &p::guid(id)))?;
                }
                deleted(FwpmSubLayerDeleteByKey0(self.0, &p::guid(p::SUBLAYER)))?;
                deleted(FwpmProviderDeleteByKey0(self.0, &p::guid(p::PROVIDER)))?;
            }
            Ok(())
        })
    }
    pub fn install(&self, policy: &Policy) -> Result<(), String> {
        self.transaction(|| {
            let mut provider_id = p::guid(p::PROVIDER);
            let mut title = wide("BandwidthLimiter experimental bidirectional TCP shaping");
            let display = FWPM_DISPLAY_DATA0 {
                name: title.as_mut_ptr(),
                description: null_mut(),
            };
            let provider = FWPM_PROVIDER0 {
                providerKey: provider_id,
                displayData: display,
                ..Default::default()
            };
            let sublayer = FWPM_SUBLAYER0 {
                subLayerKey: p::guid(p::SUBLAYER),
                displayData: display,
                providerKey: &mut provider_id,
                weight: 0x8000,
                ..Default::default()
            };
            // SAFETY: all structures/strings/condition blobs stay live until synchronous add calls return.
            unsafe {
                checked(
                    FwpmProviderAdd0(self.0, &provider, null_mut()),
                    "Add provider",
                )?;
                checked(
                    FwpmSubLayerAdd0(self.0, &sublayer, null_mut()),
                    "Add sublayer",
                )?;
                let layers = [
                    FWPM_LAYER_ALE_FLOW_ESTABLISHED_V4,
                    FWPM_LAYER_ALE_FLOW_ESTABLISHED_V6,
                    FWPM_LAYER_STREAM_V4,
                    FWPM_LAYER_STREAM_V6,
                ];
                for (index, id) in p::CALLOUTS.iter().enumerate() {
                    let callout = FWPM_CALLOUT0 {
                        calloutKey: p::guid(*id),
                        displayData: display,
                        providerKey: &mut provider_id,
                        applicableLayer: layers[index],
                        ..Default::default()
                    };
                    checked(
                        FwpmCalloutAdd0(self.0, &callout, null_mut(), null_mut()),
                        "Add management callout",
                    )?;
                }
                for ipv6 in [false, true] {
                    let index = usize::from(ipv6);
                    let filter = filter(
                        p::STREAM_FILTERS[index],
                        layers[index + 2],
                        p::CALLOUTS[index + 2],
                        &mut provider_id,
                        display,
                        FWP_ACTION_CALLOUT_UNKNOWN,
                        0,
                    );
                    checked(
                        FwpmFilterAdd0(self.0, &filter, null_mut(), null_mut()),
                        "Add conditional stream filter",
                    )?;
                }
                for (slot, rule) in policy.rules.iter().enumerate() {
                    let path = wide(&rule.executable);
                    let mut blob = null_mut();
                    checked(
                        FwpmGetAppIdFromFileName0(path.as_ptr(), &mut blob),
                        "Resolve executable WFP app ID",
                    )?;
                    let result: Result<(), String> = (|| {
                        let mut conditions = [
                            FWPM_FILTER_CONDITION0 {
                                fieldKey: FWPM_CONDITION_ALE_APP_ID,
                                matchType: FWP_MATCH_EQUAL,
                                conditionValue: FWP_CONDITION_VALUE0 {
                                    r#type: FWP_BYTE_BLOB_TYPE,
                                    Anonymous: FWP_CONDITION_VALUE0_0 { byteBlob: blob },
                                },
                            },
                            FWPM_FILTER_CONDITION0 {
                                fieldKey: FWPM_CONDITION_IP_PROTOCOL,
                                matchType: FWP_MATCH_EQUAL,
                                conditionValue: FWP_CONDITION_VALUE0 {
                                    r#type: FWP_UINT8,
                                    Anonymous: FWP_CONDITION_VALUE0_0 { uint8: 6 },
                                },
                            },
                        ];
                        for ipv6 in [false, true] {
                            let index = usize::from(ipv6);
                            let mut f = filter(
                                p::app_filter(slot, ipv6),
                                layers[index],
                                p::CALLOUTS[index],
                                &mut provider_id,
                                display,
                                FWP_ACTION_CALLOUT_INSPECTION,
                                slot as u64 + 1,
                            );
                            f.numFilterConditions = conditions.len() as u32;
                            f.filterCondition = conditions.as_mut_ptr();
                            checked(
                                FwpmFilterAdd0(self.0, &f, null_mut(), null_mut()),
                                "Add executable ALE filter",
                            )?;
                        }
                        Ok(())
                    })();
                    FwpmFreeMemory0((&mut blob as *mut *mut FWP_BYTE_BLOB).cast::<*mut c_void>());
                    result?;
                }
            }
            Ok(())
        })
    }
}
fn filter(
    id: u32,
    layer: GUID,
    callout: u32,
    provider: *mut GUID,
    display: FWPM_DISPLAY_DATA0,
    action: FWP_ACTION_TYPE,
    context: u64,
) -> FWPM_FILTER0 {
    FWPM_FILTER0 {
        filterKey: p::guid(id),
        displayData: display,
        flags: FWPM_FILTER_FLAG_PERMIT_IF_CALLOUT_UNREGISTERED,
        providerKey: provider,
        layerKey: layer,
        subLayerKey: p::guid(p::SUBLAYER),
        weight: FWP_VALUE0 {
            r#type: FWP_UINT8,
            Anonymous: FWP_VALUE0_0 { uint8: 128 },
        },
        action: FWPM_ACTION0 {
            r#type: action,
            Anonymous: FWPM_ACTION0_0 {
                calloutKey: p::guid(callout),
            },
        },
        Anonymous: FWPM_FILTER0_0 {
            rawContext: context,
        },
        ..Default::default()
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: closes this engine handle. Non-dynamic filter objects intentionally survive a crash.
        unsafe {
            FwpmEngineClose0(self.0);
        }
    }
}
