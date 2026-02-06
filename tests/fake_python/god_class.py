"""A 'God Class' that does way too many unrelated things."""

import json
import os
import smtplib
import sqlite3
from datetime import datetime
from email.mime.text import MIMEText
from typing import Any, Optional


class ApplicationManager:
    """
    This class violates Single Responsibility Principle badly.
    It handles:
    - Database operations
    - Email sending
    - File I/O
    - Configuration management
    - User session management
    - Logging
    - Caching
    - API calls
    - Report generation
    """

    def __init__(self, config_path: str = "config.json"):
        self.config_path = config_path
        self.config = {}
        self.db_connection = None
        self.cache = {}
        self.sessions = {}
        self.log_file = None
        self.smtp_connection = None
        
        self._load_config()
        self._init_database()
        self._init_logging()

    # ========== Configuration Management ==========
    
    def _load_config(self):
        if os.path.exists(self.config_path):
            with open(self.config_path, 'r') as f:
                self.config = json.load(f)
        else:
            self.config = self._default_config()
            self.save_config()

    def _default_config(self) -> dict:
        return {
            "database": {"path": "app.db"},
            "email": {"host": "localhost", "port": 25},
            "logging": {"level": "INFO", "file": "app.log"},
            "cache": {"enabled": True, "ttl": 300}
        }

    def save_config(self):
        with open(self.config_path, 'w') as f:
            json.dump(self.config, f, indent=2)

    def get_config(self, key: str, default: Any = None) -> Any:
        keys = key.split(".")
        value = self.config
        for k in keys:
            if isinstance(value, dict) and k in value:
                value = value[k]
            else:
                return default
        return value

    def set_config(self, key: str, value: Any):
        keys = key.split(".")
        config = self.config
        for k in keys[:-1]:
            if k not in config:
                config[k] = {}
            config = config[k]
        config[keys[-1]] = value

    # ========== Database Operations ==========
    
    def _init_database(self):
        db_path = self.get_config("database.path", "app.db")
        self.db_connection = sqlite3.connect(db_path)
        self._create_tables()

    def _create_tables(self):
        cursor = self.db_connection.cursor()
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY,
                username TEXT UNIQUE,
                email TEXT,
                password_hash TEXT,
                created_at TEXT
            )
        """)
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS logs (
                id INTEGER PRIMARY KEY,
                timestamp TEXT,
                level TEXT,
                message TEXT
            )
        """)
        self.db_connection.commit()

    def execute_query(self, query: str, params: tuple = ()) -> list:
        cursor = self.db_connection.cursor()
        cursor.execute(query, params)
        return cursor.fetchall()

    def execute_insert(self, table: str, data: dict) -> int:
        columns = ", ".join(data.keys())
        placeholders = ", ".join(["?" for _ in data])
        query = f"INSERT INTO {table} ({columns}) VALUES ({placeholders})"
        cursor = self.db_connection.cursor()
        cursor.execute(query, tuple(data.values()))
        self.db_connection.commit()
        return cursor.lastrowid

    def execute_update(self, table: str, data: dict, where: dict) -> int:
        set_clause = ", ".join([f"{k} = ?" for k in data])
        where_clause = " AND ".join([f"{k} = ?" for k in where])
        query = f"UPDATE {table} SET {set_clause} WHERE {where_clause}"
        cursor = self.db_connection.cursor()
        cursor.execute(query, tuple(data.values()) + tuple(where.values()))
        self.db_connection.commit()
        return cursor.rowcount

    def execute_delete(self, table: str, where: dict) -> int:
        where_clause = " AND ".join([f"{k} = ?" for k in where])
        query = f"DELETE FROM {table} WHERE {where_clause}"
        cursor = self.db_connection.cursor()
        cursor.execute(query, tuple(where.values()))
        self.db_connection.commit()
        return cursor.rowcount

    # ========== Email Operations ==========
    
    def _connect_smtp(self):
        if self.smtp_connection is None:
            host = self.get_config("email.host", "localhost")
            port = self.get_config("email.port", 25)
            self.smtp_connection = smtplib.SMTP(host, port)

    def send_email(self, to: str, subject: str, body: str, from_addr: str = "noreply@example.com"):
        self._connect_smtp()
        msg = MIMEText(body)
        msg["Subject"] = subject
        msg["From"] = from_addr
        msg["To"] = to
        self.smtp_connection.sendmail(from_addr, [to], msg.as_string())
        self.log("INFO", f"Email sent to {to}: {subject}")

    def send_welcome_email(self, user_email: str, username: str):
        subject = "Welcome to our platform!"
        body = f"Hello {username},\n\nWelcome to our platform!\n\nBest regards"
        self.send_email(user_email, subject, body)

    def send_password_reset_email(self, user_email: str, reset_token: str):
        subject = "Password Reset Request"
        body = f"Click here to reset: https://example.com/reset?token={reset_token}"
        self.send_email(user_email, subject, body)

    # ========== Logging ==========
    
    def _init_logging(self):
        log_file = self.get_config("logging.file", "app.log")
        self.log_file = open(log_file, "a")

    def log(self, level: str, message: str):
        timestamp = datetime.now().isoformat()
        log_entry = f"[{timestamp}] [{level}] {message}"
        
        # Write to file
        if self.log_file:
            self.log_file.write(log_entry + "\n")
            self.log_file.flush()
        
        # Write to database
        self.execute_insert("logs", {
            "timestamp": timestamp,
            "level": level,
            "message": message
        })
        
        # Print to console
        print(log_entry)

    def log_info(self, message: str):
        self.log("INFO", message)

    def log_error(self, message: str):
        self.log("ERROR", message)

    def log_warning(self, message: str):
        self.log("WARNING", message)

    # ========== Caching ==========
    
    def cache_get(self, key: str) -> Optional[Any]:
        if not self.get_config("cache.enabled", True):
            return None
        if key in self.cache:
            entry = self.cache[key]
            if datetime.now().timestamp() - entry["timestamp"] < self.get_config("cache.ttl", 300):
                return entry["value"]
            else:
                del self.cache[key]
        return None

    def cache_set(self, key: str, value: Any):
        if self.get_config("cache.enabled", True):
            self.cache[key] = {
                "value": value,
                "timestamp": datetime.now().timestamp()
            }

    def cache_clear(self):
        self.cache = {}

    # ========== Session Management ==========
    
    def create_session(self, user_id: int) -> str:
        import hashlib
        import secrets
        token = secrets.token_hex(32)
        self.sessions[token] = {
            "user_id": user_id,
            "created_at": datetime.now().timestamp(),
            "last_access": datetime.now().timestamp()
        }
        self.log_info(f"Session created for user {user_id}")
        return token

    def validate_session(self, token: str) -> Optional[int]:
        if token in self.sessions:
            session = self.sessions[token]
            session["last_access"] = datetime.now().timestamp()
            return session["user_id"]
        return None

    def destroy_session(self, token: str):
        if token in self.sessions:
            user_id = self.sessions[token]["user_id"]
            del self.sessions[token]
            self.log_info(f"Session destroyed for user {user_id}")

    # ========== User Management (yes, this too!) ==========
    
    def create_user(self, username: str, email: str, password: str) -> int:
        import hashlib
        password_hash = hashlib.sha256(password.encode()).hexdigest()
        user_id = self.execute_insert("users", {
            "username": username,
            "email": email,
            "password_hash": password_hash,
            "created_at": datetime.now().isoformat()
        })
        self.send_welcome_email(email, username)
        self.log_info(f"User created: {username}")
        return user_id

    def authenticate_user(self, username: str, password: str) -> Optional[str]:
        import hashlib
        password_hash = hashlib.sha256(password.encode()).hexdigest()
        results = self.execute_query(
            "SELECT id FROM users WHERE username = ? AND password_hash = ?",
            (username, password_hash)
        )
        if results:
            user_id = results[0][0]
            token = self.create_session(user_id)
            self.log_info(f"User authenticated: {username}")
            return token
        self.log_warning(f"Failed authentication attempt: {username}")
        return None

    # ========== Report Generation (why not!) ==========
    
    def generate_user_report(self) -> str:
        users = self.execute_query("SELECT id, username, email, created_at FROM users")
        report = "USER REPORT\n" + "=" * 40 + "\n"
        for user in users:
            report += f"ID: {user[0]}, Username: {user[1]}, Email: {user[2]}\n"
        report += "=" * 40 + f"\nTotal users: {len(users)}"
        return report

    def generate_log_report(self, level: Optional[str] = None) -> str:
        if level:
            logs = self.execute_query(
                "SELECT timestamp, level, message FROM logs WHERE level = ? ORDER BY timestamp DESC LIMIT 100",
                (level,)
            )
        else:
            logs = self.execute_query(
                "SELECT timestamp, level, message FROM logs ORDER BY timestamp DESC LIMIT 100"
            )
        report = "LOG REPORT\n" + "=" * 40 + "\n"
        for log in logs:
            report += f"[{log[0]}] [{log[1]}] {log[2]}\n"
        return report

    # ========== Cleanup ==========
    
    def close(self):
        if self.db_connection:
            self.db_connection.close()
        if self.log_file:
            self.log_file.close()
        if self.smtp_connection:
            self.smtp_connection.quit()

