"""API handler with big try/except blocks and functions that do too much."""

import json
import logging
import time
from datetime import datetime


logger = logging.getLogger(__name__)


def handle_api_request(request):
    """
    This function does way too many things:
    - Request parsing
    - Authentication
    - Authorization  
    - Rate limiting
    - Input validation
    - Business logic routing
    - Response formatting
    - Error handling
    - Logging
    - Metrics
    """
    start_time = time.time()
    request_id = f"req_{int(start_time * 1000)}"
    
    try:
        # Parse request
        if request is None:
            raise ValueError("Request cannot be None")
        
        method = request.get("method", "GET")
        path = request.get("path", "/")
        headers = request.get("headers", {})
        body = request.get("body")
        query_params = request.get("query_params", {})
        
        logger.info(f"[{request_id}] {method} {path}")
        
        # Authentication
        auth_header = headers.get("Authorization", "")
        user = None
        
        if auth_header:
            if auth_header.startswith("Bearer "):
                token = auth_header[7:]
                # Simulate token validation
                if len(token) > 10:
                    user = {
                        "id": 1,
                        "username": "testuser",
                        "roles": ["user"],
                        "permissions": ["read", "write"]
                    }
                else:
                    return {
                        "status": 401,
                        "body": {"error": "Invalid token"},
                        "headers": {"Content-Type": "application/json"}
                    }
            elif auth_header.startswith("Basic "):
                import base64
                credentials = base64.b64decode(auth_header[6:]).decode()
                username, password = credentials.split(":", 1)
                if username == "admin" and password == "secret":
                    user = {
                        "id": 0,
                        "username": "admin",
                        "roles": ["admin", "user"],
                        "permissions": ["read", "write", "delete", "admin"]
                    }
                else:
                    return {
                        "status": 401,
                        "body": {"error": "Invalid credentials"},
                        "headers": {"Content-Type": "application/json"}
                    }
        
        # Rate limiting (simplified)
        client_ip = headers.get("X-Forwarded-For", "127.0.0.1")
        # In real code, check rate limit here
        
        # Route to handler
        if path == "/api/users":
            if method == "GET":
                if user is None:
                    return {
                        "status": 401,
                        "body": {"error": "Authentication required"},
                        "headers": {"Content-Type": "application/json"}
                    }
                # Simulate fetching users
                users = [
                    {"id": 1, "username": "alice", "email": "alice@example.com"},
                    {"id": 2, "username": "bob", "email": "bob@example.com"},
                ]
                return {
                    "status": 200,
                    "body": {"users": users, "total": len(users)},
                    "headers": {"Content-Type": "application/json"}
                }
            elif method == "POST":
                if user is None:
                    return {
                        "status": 401,
                        "body": {"error": "Authentication required"},
                        "headers": {"Content-Type": "application/json"}
                    }
                if "admin" not in user.get("roles", []):
                    return {
                        "status": 403,
                        "body": {"error": "Admin access required"},
                        "headers": {"Content-Type": "application/json"}
                    }
                if body is None:
                    return {
                        "status": 400,
                        "body": {"error": "Request body required"},
                        "headers": {"Content-Type": "application/json"}
                    }
                # Validate user data
                if "username" not in body:
                    return {
                        "status": 400,
                        "body": {"error": "Username is required"},
                        "headers": {"Content-Type": "application/json"}
                    }
                if "email" not in body:
                    return {
                        "status": 400,
                        "body": {"error": "Email is required"},
                        "headers": {"Content-Type": "application/json"}
                    }
                # Create user
                new_user = {
                    "id": 3,
                    "username": body["username"],
                    "email": body["email"],
                    "created_at": datetime.now().isoformat()
                }
                return {
                    "status": 201,
                    "body": new_user,
                    "headers": {"Content-Type": "application/json"}
                }
        
        elif path.startswith("/api/users/"):
            user_id = path.split("/")[-1]
            if method == "GET":
                return {
                    "status": 200,
                    "body": {"id": user_id, "username": f"user_{user_id}"},
                    "headers": {"Content-Type": "application/json"}
                }
            elif method == "DELETE":
                if user is None or "admin" not in user.get("roles", []):
                    return {
                        "status": 403,
                        "body": {"error": "Admin access required"},
                        "headers": {"Content-Type": "application/json"}
                    }
                return {
                    "status": 204,
                    "body": None,
                    "headers": {}
                }
        
        elif path == "/api/health":
            return {
                "status": 200,
                "body": {"status": "healthy", "timestamp": datetime.now().isoformat()},
                "headers": {"Content-Type": "application/json"}
            }
        
        else:
            return {
                "status": 404,
                "body": {"error": f"Not found: {path}"},
                "headers": {"Content-Type": "application/json"}
            }
    
    except ValueError as e:
        logger.error(f"[{request_id}] ValueError: {e}")
        return {
            "status": 400,
            "body": {"error": str(e)},
            "headers": {"Content-Type": "application/json"}
        }
    except KeyError as e:
        logger.error(f"[{request_id}] KeyError: {e}")
        return {
            "status": 400,
            "body": {"error": f"Missing field: {e}"},
            "headers": {"Content-Type": "application/json"}
        }
    except json.JSONDecodeError as e:
        logger.error(f"[{request_id}] JSONDecodeError: {e}")
        return {
            "status": 400,
            "body": {"error": "Invalid JSON"},
            "headers": {"Content-Type": "application/json"}
        }
    except PermissionError as e:
        logger.error(f"[{request_id}] PermissionError: {e}")
        return {
            "status": 403,
            "body": {"error": "Permission denied"},
            "headers": {"Content-Type": "application/json"}
        }
    except TimeoutError as e:
        logger.error(f"[{request_id}] TimeoutError: {e}")
        return {
            "status": 504,
            "body": {"error": "Request timeout"},
            "headers": {"Content-Type": "application/json"}
        }
    except ConnectionError as e:
        logger.error(f"[{request_id}] ConnectionError: {e}")
        return {
            "status": 503,
            "body": {"error": "Service unavailable"},
            "headers": {"Content-Type": "application/json"}
        }
    except Exception as e:
        logger.exception(f"[{request_id}] Unexpected error: {e}")
        return {
            "status": 500,
            "body": {"error": "Internal server error"},
            "headers": {"Content-Type": "application/json"}
        }
    finally:
        elapsed = time.time() - start_time
        logger.info(f"[{request_id}] Completed in {elapsed:.3f}s")


def process_batch_operations(operations, context):
    """Another function with a massive try/except and too many responsibilities."""
    results = []
    errors = []
    stats = {
        "total": len(operations),
        "success": 0,
        "failed": 0,
        "skipped": 0
    }
    
    try:
        # Validate context
        if not context:
            raise ValueError("Context is required")
        if not context.get("user"):
            raise ValueError("User context is required")
        if not context.get("database"):
            raise ValueError("Database connection is required")
        
        db = context["database"]
        user = context["user"]
        
        # Check permissions
        if "batch_operations" not in user.get("permissions", []):
            raise PermissionError("User lacks batch operation permission")
        
        # Process each operation
        for i, op in enumerate(operations):
            try:
                op_type = op.get("type")
                op_data = op.get("data", {})
                op_target = op.get("target")
                
                if op_type == "create":
                    # Validate create data
                    if not op_data:
                        raise ValueError(f"Operation {i}: No data for create")
                    if not op_target:
                        raise ValueError(f"Operation {i}: No target for create")
                    
                    # Insert into database
                    result = db.insert(op_target, op_data)
                    results.append({"index": i, "status": "created", "id": result.get("id")})
                    stats["success"] += 1
                
                elif op_type == "update":
                    if not op_data:
                        raise ValueError(f"Operation {i}: No data for update")
                    if not op_target:
                        raise ValueError(f"Operation {i}: No target for update")
                    if "id" not in op_data:
                        raise ValueError(f"Operation {i}: No ID for update")
                    
                    result = db.update(op_target, {"id": op_data["id"]}, op_data)
                    results.append({"index": i, "status": "updated", "id": op_data["id"]})
                    stats["success"] += 1
                
                elif op_type == "delete":
                    if not op_target:
                        raise ValueError(f"Operation {i}: No target for delete")
                    if "id" not in op_data:
                        raise ValueError(f"Operation {i}: No ID for delete")
                    
                    db.delete(op_target, {"id": op_data["id"]})
                    results.append({"index": i, "status": "deleted", "id": op_data["id"]})
                    stats["success"] += 1
                
                elif op_type == "skip":
                    results.append({"index": i, "status": "skipped"})
                    stats["skipped"] += 1
                
                else:
                    raise ValueError(f"Operation {i}: Unknown operation type: {op_type}")
            
            except ValueError as e:
                errors.append({"index": i, "error": str(e)})
                stats["failed"] += 1
            except KeyError as e:
                errors.append({"index": i, "error": f"Missing key: {e}"})
                stats["failed"] += 1
            except Exception as e:
                errors.append({"index": i, "error": f"Unexpected: {e}"})
                stats["failed"] += 1
        
        return {
            "success": stats["failed"] == 0,
            "results": results,
            "errors": errors,
            "stats": stats
        }
    
    except ValueError as e:
        logger.error(f"Batch validation error: {e}")
        return {
            "success": False,
            "results": [],
            "errors": [{"index": -1, "error": str(e)}],
            "stats": stats
        }
    except PermissionError as e:
        logger.error(f"Batch permission error: {e}")
        return {
            "success": False,
            "results": [],
            "errors": [{"index": -1, "error": str(e)}],
            "stats": stats
        }
    except ConnectionError as e:
        logger.error(f"Batch connection error: {e}")
        return {
            "success": False,
            "results": [],
            "errors": [{"index": -1, "error": "Database connection failed"}],
            "stats": stats
        }
    except Exception as e:
        logger.exception(f"Batch unexpected error: {e}")
        return {
            "success": False,
            "results": [],
            "errors": [{"index": -1, "error": "Internal error"}],
            "stats": stats
        }

