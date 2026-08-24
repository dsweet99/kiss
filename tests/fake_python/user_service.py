

class UserService:

    def __init__(self, db):
        self.db = db
        self.cache = {}
        self.logger = None

    def create_user(self, name, email, role="member"):
        if not name or not email:
            raise ValueError("Name and email are required")
        if "@" not in email:
            raise ValueError("Invalid email address")
        user = {
            "name": name,
            "email": email,
            "role": role,
            "active": True,
        }
        result = self.db.insert("users", user)
        self.cache[result["id"]] = result
        return result

    def update_user(self, user_id, name, email, role="member"):
        if not name or not email:
            raise ValueError("Name and email are required")
        if "@" not in email:
            raise ValueError("Invalid email address")
        user = {
            "name": name,
            "email": email,
            "role": role,
            "active": True,
        }
        result = self.db.update("users", user_id, user)
        self.cache[result["id"]] = result
        return result

    def delete_user(self, user_id):
        if user_id not in self.cache:
            existing = self.db.find("users", user_id)
            if not existing:
                raise ValueError("User not found")
        self.db.delete("users", user_id)
        if user_id in self.cache:
            del self.cache[user_id]
        return True

    def get_user(self, user_id):
        if user_id in self.cache:
            return self.cache[user_id]
        user = self.db.find("users", user_id)
        if user:
            self.cache[user_id] = user
        return user

    def list_active_users(self):
        users = self.db.query("users", {"active": True})
        result = []
        for user in users:
            if user.get("active"):
                result.append(user)
                self.cache[user["id"]] = user
        return result

    def list_inactive_users(self):
        users = self.db.query("users", {"active": False})
        result = []
        for user in users:
            if not user.get("active"):
                result.append(user)
                self.cache[user["id"]] = user
        return result

    def create_admin(self, name, email, role="admin"):
        if not name or not email:
            raise ValueError("Name and email are required")
        if "@" not in email:
            raise ValueError("Invalid email address")
        user = {
            "name": name,
            "email": email,
            "role": role,
            "active": True,
        }
        result = self.db.insert("users", user)
        self.cache[result["id"]] = result
        return result

    def update_admin(self, user_id, name, email, role="admin"):
        if not name or not email:
            raise ValueError("Name and email are required")
        if "@" not in email:
            raise ValueError("Invalid email address")
        user = {
            "name": name,
            "email": email,
            "role": role,
            "active": True,
        }
        result = self.db.update("users", user_id, user)
        self.cache[result["id"]] = result
        return result

    def search_users_by_name(self, name):
        all_users = self.db.query("users", {})
        matches = []
        for user in all_users:
            if user.get("name", "").startswith(name):
                matches.append(user)
                self.cache[user["id"]] = user
        return matches

    def search_users_by_email(self, email):
        all_users = self.db.query("users", {})
        matches = []
        for user in all_users:
            if user.get("email", "").endswith(email):
                matches.append(user)
                self.cache[user["id"]] = user
        return matches
