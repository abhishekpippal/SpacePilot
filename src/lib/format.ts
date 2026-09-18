export const bytes = (value:number) => { if (!value) return "0 B"; const units=["B","KB","MB","GB","TB"]; const i=Math.min(Math.floor(Math.log(value)/Math.log(1024)),4); return `${(value/1024**i).toFixed(i ? 1 : 0)} ${units[i]}`; };
export const date = (value:string|null) => value ? new Intl.DateTimeFormat(undefined,{dateStyle:"medium"}).format(new Date(value)) : "Unknown";
