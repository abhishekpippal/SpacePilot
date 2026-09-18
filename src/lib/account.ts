import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

export type EntitlementState="ONLINE_VERIFIED"|"OFFLINE_GRACE"|"EXPIRED"|"BACKEND_UNAVAILABLE";
export interface Capabilities { canUseAiCopilot:boolean; canUseFullSpaceRescue:boolean; canUseFullTimeline:boolean; canUseDeveloperStorage:boolean; canUseFullDuplicates:boolean }
export interface AccountUser { id:string; email:string; email_verified:boolean; status:string }
export interface Device { id:string; device_public_id:string; device_name:string; platform:string; architecture:string; app_version:string; revoked_at:string|null }
export interface Entitlement { plan:string; status:string; capabilities:Record<string,boolean>; device_limit:number; ai_allowance:number; ai_usage:number; grace_until:string; assertion:string }
export interface AccountSnapshot { user:AccountUser; devices:Device[]; entitlement:Entitlement; deviceId:string }

export const signedOutCapabilities=():Capabilities=>({canUseAiCopilot:false,canUseFullSpaceRescue:false,canUseFullTimeline:false,canUseDeveloperStorage:false,canUseFullDuplicates:false});
export function capabilitiesFrom(value:Record<string,boolean>|null|undefined):Capabilities{return{
 canUseAiCopilot:Boolean(value?.can_use_ai_copilot),canUseFullSpaceRescue:Boolean(value?.can_use_full_space_rescue),canUseFullTimeline:Boolean(value?.can_use_full_timeline),canUseDeveloperStorage:Boolean(value?.can_use_developer_storage),canUseFullDuplicates:Boolean(value?.can_use_full_duplicates)
}}
const request=<T>(method:string,path:string,body?:unknown)=>invoke<T>("backend_request",{method,path,body:body??null});
export const sessionStatus=()=>invoke<boolean>("secure_session_status");
export const authenticate=(action:"login"|"signup",email:string,password:string)=>invoke("account_auth",{action,email,password});
export const requestPasswordReset=(email:string)=>request("POST","/v1/auth/password-reset/request",{email});
export const deviceId=()=>invoke<string>("stable_device_id");
export async function loadAccount():Promise<AccountSnapshot>{const id=await deviceId();const user=await request<AccountUser>("GET","/v1/auth/me");await request("POST","/v1/devices/register",{device_public_id:id,device_name:"SpacePilot Windows PC",platform:"windows",architecture:"x86_64",app_version:"0.6.0"});const [devices,entitlement]=await Promise.all([request<Device[]>("GET","/v1/devices"),request<Entitlement>("GET",`/v1/entitlement?device_public_id=${encodeURIComponent(id)}`)]);await invoke("cache_entitlement",{assertion:entitlement.assertion});return{user,devices,entitlement,deviceId:id}}
export async function revokeDevice(id:string){await request("DELETE",`/v1/devices/${encodeURIComponent(id)}`)}
export async function openCheckout(plan:"PRO_MONTHLY"|"PRO_ANNUAL"){const result=await request<{url:string}>("POST","/v1/billing/checkout",{plan});await openUrl(result.url)}
export async function openBillingPortal(){const result=await request<{url:string}>("POST","/v1/billing/portal");await openUrl(result.url)}
export async function logout(){await invoke("account_logout")}
export function messageFor(code:unknown):string{const key=String(code);return({AUTH_INVALID:"Email or password is incorrect.",ACCOUNT_EXISTS:"An account already exists for this email.",RATE_LIMITED:"Too many attempts. Please wait and try again.",DEVICE_LIMIT_REACHED:"Your plan's device limit has been reached.",BACKEND_UNAVAILABLE:"SpacePilot services are unavailable. Local scanning still works.",AI_QUOTA_EXCEEDED:"Your AI allowance for this month has been used."}as Record<string,string>)[key]??"The request could not be completed."}
