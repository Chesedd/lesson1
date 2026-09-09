import { invoke } from '@tauri-apps/api/core';
import type { Lesson, Progress } from '../types';
export interface LearningService { loadLesson(id:string):Promise<Lesson>; loadLessonProgress(id:string):Promise<Progress>; }
export const learningService:LearningService={loadLesson:(lessonId)=>invoke<Lesson>('get_lesson',{lessonId}),loadLessonProgress:(lessonId)=>invoke<Progress>('get_lesson_progress',{lessonId})};
