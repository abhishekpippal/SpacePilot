import { useEffect, useState } from "react";

export type AiProviderId="none"|"openrouter"|"groq"|"spacepilot_hosted";
export interface SettingsState { largeFileThreshold:number; confirmDeletion:boolean; timelineRetentionDays:30|90|180; aiCopilotEnabled:boolean; aiProvider:AiProviderId; aiModel:string }
export const defaultSettings:SettingsState={largeFileThreshold:100*1024*1024,confirmDeletion:true,timelineRetentionDays:90,aiCopilotEnabled:false,aiProvider:"none",aiModel:"automatic"};

export function parseSettings(raw:string|null):SettingsState{
  if(!raw)return defaultSettings;
  try{
    const value:unknown=JSON.parse(raw);
    if(!value||typeof value!=="object"||Array.isArray(value))return defaultSettings;
    const candidate=value as Record<string,unknown>;
    const threshold=typeof candidate.largeFileThreshold==="number"&&Number.isFinite(candidate.largeFileThreshold)
      ?Math.min(Math.max(Math.round(candidate.largeFileThreshold),1024*1024),100*1024**4)
      :defaultSettings.largeFileThreshold;
    const confirmDeletion=typeof candidate.confirmDeletion==="boolean"
      ?candidate.confirmDeletion
      :defaultSettings.confirmDeletion;
    const timelineRetentionDays=candidate.timelineRetentionDays===30||candidate.timelineRetentionDays===180?candidate.timelineRetentionDays:90;
    const aiCopilotEnabled=typeof candidate.aiCopilotEnabled==="boolean"?candidate.aiCopilotEnabled:false;
    const aiProvider=["none","openrouter","groq","spacepilot_hosted"].includes(String(candidate.aiProvider))?candidate.aiProvider as AiProviderId:defaultSettings.aiProvider;
    const aiModel=typeof candidate.aiModel==="string"&&candidate.aiModel.length<120?candidate.aiModel:defaultSettings.aiModel;
    return {largeFileThreshold:threshold,confirmDeletion,timelineRetentionDays,aiCopilotEnabled,aiProvider,aiModel};
  }catch{return defaultSettings}
}

export function useSettings(){
  const [settings,setSettings]=useState<SettingsState>(()=>{
    try{return parseSettings(localStorage.getItem("spacepilot.settings"))}catch{return defaultSettings}
  });
  useEffect(()=>{try{localStorage.setItem("spacepilot.settings",JSON.stringify(settings))}catch{/* Defaults remain usable if storage is unavailable. */}},[settings]);
  return [settings,setSettings] as const;
}
