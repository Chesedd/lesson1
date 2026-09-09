export type TheoryPart={type:'heading';text:string}|{type:'paragraph';text:string}|{type:'code';language:'python';code:string};
export interface Asset{id:string;filename:string;media_type:'text/csv';description:string}
export interface Exercise{id:string;title:string;difficulty:'basic'|'intermediate'|'advanced';statement:string;starter_code:string;hints:string[];public_examples:{input:string;output:string}[];assets:Asset[];order:number;runtime:'python'}
export interface Block{id:string;title:string;order:number;theory:{parts:TheoryPart[]};exercises:Exercise[]}
export interface Lesson{id:string;title:string;age_group:string;content_version:string;blocks:Block[]}
export interface Progress{lesson_id:string;completed:number;total:number;percent:number;completed_exercise_ids:string[]}
export interface RunRequest{protocol_version:1;lesson_id:string;exercise_id:string;code:string}
export interface RunResult{protocol_version:1;status:'success'|'runtime_error'|'import_error'|'syntax_error'|'timeout'|'output_limit'|'runner_error';stdout:string;stderr:string;exit_code:number|null;duration_ms:number;truncated:boolean}
