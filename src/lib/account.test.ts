import {describe,expect,it} from "vitest";
import {capabilitiesFrom,messageFor,signedOutCapabilities} from "./account";
describe("commercial account state",()=>{
 it("denies commercial capabilities while signed out",()=>expect(Object.values(signedOutCapabilities()).every(value=>!value)).toBe(true));
 it("maps server capabilities without plan-name checks",()=>expect(capabilitiesFrom({can_use_ai_copilot:true,can_use_full_timeline:false})).toEqual({canUseAiCopilot:true,canUseFullSpaceRescue:false,canUseFullTimeline:false,canUseDeveloperStorage:false,canUseFullDuplicates:false}));
 it("maps quota and backend errors to safe messages",()=>{expect(messageFor("AI_QUOTA_EXCEEDED")).toContain("allowance");expect(messageFor("BACKEND_UNAVAILABLE")).toContain("scanning")});
});
