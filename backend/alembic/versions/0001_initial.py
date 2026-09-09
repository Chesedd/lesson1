"""Initial trusted progress tables."""
from alembic import op
import sqlalchemy as sa
revision = "0001"
down_revision = None
branch_labels = None
depends_on = None

def upgrade():
    op.create_table("learner", sa.Column("id", sa.String(64), primary_key=True), sa.Column("created_at", sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False))
    op.create_table("exercise_completion", sa.Column("id", sa.Integer(), primary_key=True), sa.Column("learner_id", sa.String(64), sa.ForeignKey("learner.id", ondelete="CASCADE"), nullable=False), sa.Column("exercise_id", sa.String(120), nullable=False), sa.Column("content_version", sa.String(40), nullable=False), sa.Column("completed_at", sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False), sa.UniqueConstraint("learner_id", "exercise_id", "content_version", name="uq_completion_identity"))
    op.create_index("ix_exercise_completion_learner_id", "exercise_completion", ["learner_id"])

def downgrade():
    op.drop_table("exercise_completion")
    op.drop_table("learner")
