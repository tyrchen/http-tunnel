from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel
from typing import Optional
from datetime import datetime
import uvicorn

app = FastAPI(title="TodoMVC API")

# CORS middleware to allow frontend access
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


# Todo Model
class TodoBase(BaseModel):
    title: str
    completed: bool = False


class TodoCreate(TodoBase):
    pass


class TodoUpdate(BaseModel):
    title: Optional[str] = None
    completed: Optional[bool] = None


class Todo(TodoBase):
    id: int
    created_at: datetime
    updated_at: datetime


# In-memory storage
todos_db: dict[int, dict] = {}
next_id = 1


# Initialize with meaningful dummy data
def init_dummy_data():
    global next_id
    dummy_todos = [
        {"title": "Complete project documentation", "completed": False},
        {"title": "Review pull requests", "completed": True},
        {"title": "Write unit tests for API endpoints", "completed": False},
        {"title": "Update dependencies", "completed": True},
        {"title": "Fix bug in authentication module", "completed": False},
        {"title": "Deploy to production", "completed": False},
        {"title": "Schedule team meeting", "completed": True},
    ]

    for todo_data in dummy_todos:
        now = datetime.now()
        todos_db[next_id] = {
            "id": next_id,
            "title": todo_data["title"],
            "completed": todo_data["completed"],
            "created_at": now,
            "updated_at": now,
        }
        next_id += 1


# Initialize data on startup
init_dummy_data()


# Root endpoint
@app.get("/")
def read_root():
    return {"message": "TodoMVC API", "docs": "/docs"}


# Get all todos
@app.get("/todos", response_model=list[Todo])
def get_todos(completed: Optional[bool] = None):
    todos = list(todos_db.values())
    if completed is not None:
        todos = [t for t in todos if t["completed"] == completed]
    return todos


# Get a single todo
@app.get("/todos/{todo_id}", response_model=Todo)
def get_todo(todo_id: int):
    if todo_id not in todos_db:
        raise HTTPException(status_code=404, detail="Todo not found")
    return todos_db[todo_id]


# Create a new todo
@app.post("/todos", response_model=Todo, status_code=201)
def create_todo(todo: TodoCreate):
    global next_id
    now = datetime.now()
    new_todo = {
        "id": next_id,
        "title": todo.title,
        "completed": todo.completed,
        "created_at": now,
        "updated_at": now,
    }
    todos_db[next_id] = new_todo
    next_id += 1
    return new_todo


# Update a todo
@app.put("/todos/{todo_id}", response_model=Todo)
def update_todo(todo_id: int, todo: TodoUpdate):
    if todo_id not in todos_db:
        raise HTTPException(status_code=404, detail="Todo not found")

    stored_todo = todos_db[todo_id]

    if todo.title is not None:
        stored_todo["title"] = todo.title
    if todo.completed is not None:
        stored_todo["completed"] = todo.completed

    stored_todo["updated_at"] = datetime.now()

    return stored_todo


# Delete a todo
@app.delete("/todos/{todo_id}", status_code=204)
def delete_todo(todo_id: int):
    if todo_id not in todos_db:
        raise HTTPException(status_code=404, detail="Todo not found")
    del todos_db[todo_id]
    return None


# Delete all completed todos
@app.delete("/todos", status_code=204)
def delete_completed_todos():
    global todos_db
    todos_db = {k: v for k, v in todos_db.items() if not v["completed"]}
    return None


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=3000)
