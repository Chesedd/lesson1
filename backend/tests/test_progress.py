import pytest
from sqlalchemy.exc import IntegrityError
from app.models import ExerciseCompletion,Learner

def test_completion_unique_constraint(db_factory):
    with db_factory() as db:
        learner=Learner(id='learner');db.add(learner);db.commit()
        db.add(ExerciseCompletion(learner_id=learner.id,exercise_id='load-students-head',content_version='1.0.0'));db.commit()
        db.add(ExerciseCompletion(learner_id=learner.id,exercise_id='load-students-head',content_version='1.0.0'))
        with pytest.raises(IntegrityError): db.commit()

def test_progress_uses_trusted_rows(client,db_factory):
    client.get('/api/lessons/pandas-intro/progress');learner_id=client.cookies['learner_session']
    with db_factory() as db:
        db.add(ExerciseCompletion(learner_id=learner_id,exercise_id='students-shape',content_version='1.0.0'));db.commit()
    assert client.get('/api/lessons/pandas-intro/progress').json()=={'lesson_id':'pandas-intro','completed':1,'total':3,'percent':33,'completed_exercise_ids':['students-shape']}
