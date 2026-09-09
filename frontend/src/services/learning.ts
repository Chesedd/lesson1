import { invoke } from '@tauri-apps/api/core';
import type {Lesson,Progress,RunRequest,RunResult} from '../types';
export interface LearningService{loadLesson(id:string):Promise<Lesson>;loadLessonProgress(id:string):Promise<Progress>;runExercise(request:RunRequest):Promise<RunResult>}
export const learningService:LearningService={loadLesson:(lessonId)=>invoke('get_lesson',{lessonId}),loadLessonProgress:(lessonId)=>invoke('get_lesson_progress',{lessonId}),runExercise:(request)=>invoke('run_exercise',{request})};
