import { describe, expect, it } from "vitest";
import { defaultSettings, parseSettings } from "./useSettings";

describe("persistent settings validation",()=>{
  it("survives a serialized round trip",()=>{
    const settings={largeFileThreshold:500*1024*1024,confirmDeletion:false,timelineRetentionDays:180 as const,aiCopilotEnabled:true,aiProvider:"openrouter" as const,aiModel:"automatic"};
    expect(parseSettings(JSON.stringify(settings))).toEqual(settings);
  });

  it("uses safe defaults for missing or corrupt data",()=>{
    expect(parseSettings(null)).toEqual(defaultSettings);
    expect(parseSettings("{broken")).toEqual(defaultSettings);
  });

  it("ignores unknown and invalid fields",()=>{
    expect(parseSettings(JSON.stringify({largeFileThreshold:"huge",confirmDeletion:"no",lastPath:"C:\\private"})))
      .toEqual(defaultSettings);
  });

  it("bounds numeric thresholds",()=>{
    expect(parseSettings('{"largeFileThreshold":-5,"confirmDeletion":true}').largeFileThreshold).toBe(1024*1024);
  });
  it("accepts only supported Timeline retention periods",()=>{
    expect(parseSettings('{"timelineRetentionDays":30}').timelineRetentionDays).toBe(30);
    expect(parseSettings('{"timelineRetentionDays":45}').timelineRetentionDays).toBe(90);
  });
  it("never persists an injected API credential",()=>{
    const parsed=parseSettings('{"aiCopilotEnabled":true,"apiKey":"secret-value"}');
    expect(parsed.aiCopilotEnabled).toBe(true);
    expect("apiKey" in parsed).toBe(false);
  });
  it("accepts only supported AI providers and never stores provider keys",()=>{
    expect(parseSettings('{"aiProvider":"groq","aiModel":"llama-3.1-8b-instant"}').aiProvider).toBe("groq");
    expect(parseSettings('{"aiProvider":"nemotron","openrouterKey":"secret"}')).toEqual(defaultSettings);
  });
});
