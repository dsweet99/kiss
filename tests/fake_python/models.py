"""Too many classes in one file - should be split up."""

from datetime import datetime
from typing import Optional
import json


class User:
    """User model."""
    
    def __init__(self, id: int, username: str, email: str):
        self.id = id
        self.username = username
        self.email = email
        self.created_at = datetime.now()
        self.updated_at = None
        self.is_active = True
    
    def to_dict(self):
        return {
            "id": self.id,
            "username": self.username,
            "email": self.email,
            "created_at": self.created_at.isoformat(),
            "updated_at": self.updated_at.isoformat() if self.updated_at else None,
            "is_active": self.is_active
        }
    
    def __repr__(self):
        return f"User(id={self.id}, username={self.username})"


class Product:
    """Product model."""
    
    def __init__(self, id: int, name: str, price: float, category_id: int):
        self.id = id
        self.name = name
        self.price = price
        self.category_id = category_id
        self.description = ""
        self.stock = 0
        self.is_available = True
        self.created_at = datetime.now()
    
    def to_dict(self):
        return {
            "id": self.id,
            "name": self.name,
            "price": self.price,
            "category_id": self.category_id,
            "description": self.description,
            "stock": self.stock,
            "is_available": self.is_available,
            "created_at": self.created_at.isoformat()
        }
    
    def __repr__(self):
        return f"Product(id={self.id}, name={self.name}, price={self.price})"


class Category:
    """Category model."""
    
    def __init__(self, id: int, name: str, parent_id: Optional[int] = None):
        self.id = id
        self.name = name
        self.parent_id = parent_id
        self.description = ""
        self.is_active = True
    
    def to_dict(self):
        return {
            "id": self.id,
            "name": self.name,
            "parent_id": self.parent_id,
            "description": self.description,
            "is_active": self.is_active
        }
    
    def __repr__(self):
        return f"Category(id={self.id}, name={self.name})"


class Order:
    """Order model."""
    
    def __init__(self, id: int, user_id: int, status: str = "pending"):
        self.id = id
        self.user_id = user_id
        self.status = status
        self.items = []
        self.total = 0.0
        self.created_at = datetime.now()
        self.updated_at = None
        self.shipped_at = None
        self.delivered_at = None
    
    def add_item(self, product_id: int, quantity: int, price: float):
        self.items.append({
            "product_id": product_id,
            "quantity": quantity,
            "price": price,
            "subtotal": quantity * price
        })
        self._recalculate_total()
    
    def _recalculate_total(self):
        self.total = sum(item["subtotal"] for item in self.items)
    
    def to_dict(self):
        return {
            "id": self.id,
            "user_id": self.user_id,
            "status": self.status,
            "items": self.items,
            "total": self.total,
            "created_at": self.created_at.isoformat(),
            "updated_at": self.updated_at.isoformat() if self.updated_at else None,
            "shipped_at": self.shipped_at.isoformat() if self.shipped_at else None,
            "delivered_at": self.delivered_at.isoformat() if self.delivered_at else None
        }
    
    def __repr__(self):
        return f"Order(id={self.id}, user_id={self.user_id}, total={self.total})"


class OrderItem:
    """Order item model."""
    
    def __init__(self, id: int, order_id: int, product_id: int, quantity: int, price: float):
        self.id = id
        self.order_id = order_id
        self.product_id = product_id
        self.quantity = quantity
        self.price = price
    
    @property
    def subtotal(self):
        return self.quantity * self.price
    
    def to_dict(self):
        return {
            "id": self.id,
            "order_id": self.order_id,
            "product_id": self.product_id,
            "quantity": self.quantity,
            "price": self.price,
            "subtotal": self.subtotal
        }
    
    def __repr__(self):
        return f"OrderItem(id={self.id}, product_id={self.product_id}, qty={self.quantity})"


class Review:
    """Review model."""
    
    def __init__(self, id: int, user_id: int, product_id: int, rating: int, comment: str = ""):
        self.id = id
        self.user_id = user_id
        self.product_id = product_id
        self.rating = rating
        self.comment = comment
        self.created_at = datetime.now()
        self.is_verified = False
    
    def to_dict(self):
        return {
            "id": self.id,
            "user_id": self.user_id,
            "product_id": self.product_id,
            "rating": self.rating,
            "comment": self.comment,
            "created_at": self.created_at.isoformat(),
            "is_verified": self.is_verified
        }
    
    def __repr__(self):
        return f"Review(id={self.id}, rating={self.rating})"


class Address:
    """Address model."""
    
    def __init__(self, id: int, user_id: int, street: str, city: str, state: str, zip_code: str, country: str):
        self.id = id
        self.user_id = user_id
        self.street = street
        self.city = city
        self.state = state
        self.zip_code = zip_code
        self.country = country
        self.is_default = False
    
    def to_dict(self):
        return {
            "id": self.id,
            "user_id": self.user_id,
            "street": self.street,
            "city": self.city,
            "state": self.state,
            "zip_code": self.zip_code,
            "country": self.country,
            "is_default": self.is_default
        }
    
    def format_full(self):
        return f"{self.street}\n{self.city}, {self.state} {self.zip_code}\n{self.country}"
    
    def __repr__(self):
        return f"Address(id={self.id}, city={self.city})"


class Payment:
    """Payment model."""
    
    def __init__(self, id: int, order_id: int, amount: float, method: str):
        self.id = id
        self.order_id = order_id
        self.amount = amount
        self.method = method
        self.status = "pending"
        self.transaction_id = None
        self.created_at = datetime.now()
        self.processed_at = None
    
    def to_dict(self):
        return {
            "id": self.id,
            "order_id": self.order_id,
            "amount": self.amount,
            "method": self.method,
            "status": self.status,
            "transaction_id": self.transaction_id,
            "created_at": self.created_at.isoformat(),
            "processed_at": self.processed_at.isoformat() if self.processed_at else None
        }
    
    def __repr__(self):
        return f"Payment(id={self.id}, amount={self.amount}, status={self.status})"


class Coupon:
    """Coupon model."""
    
    def __init__(self, id: int, code: str, discount_type: str, discount_value: float):
        self.id = id
        self.code = code
        self.discount_type = discount_type  # 'percentage' or 'fixed'
        self.discount_value = discount_value
        self.min_order_amount = 0.0
        self.max_uses = None
        self.uses = 0
        self.expires_at = None
        self.is_active = True
    
    def apply_discount(self, order_total: float) -> float:
        if order_total < self.min_order_amount:
            return order_total
        if self.discount_type == "percentage":
            return order_total * (1 - self.discount_value / 100)
        else:
            return max(0, order_total - self.discount_value)
    
    def to_dict(self):
        return {
            "id": self.id,
            "code": self.code,
            "discount_type": self.discount_type,
            "discount_value": self.discount_value,
            "min_order_amount": self.min_order_amount,
            "max_uses": self.max_uses,
            "uses": self.uses,
            "expires_at": self.expires_at.isoformat() if self.expires_at else None,
            "is_active": self.is_active
        }
    
    def __repr__(self):
        return f"Coupon(code={self.code}, discount={self.discount_value})"


class Notification:
    """Notification model."""
    
    def __init__(self, id: int, user_id: int, title: str, message: str, notification_type: str = "info"):
        self.id = id
        self.user_id = user_id
        self.title = title
        self.message = message
        self.notification_type = notification_type
        self.is_read = False
        self.created_at = datetime.now()
        self.read_at = None
    
    def mark_as_read(self):
        self.is_read = True
        self.read_at = datetime.now()
    
    def to_dict(self):
        return {
            "id": self.id,
            "user_id": self.user_id,
            "title": self.title,
            "message": self.message,
            "notification_type": self.notification_type,
            "is_read": self.is_read,
            "created_at": self.created_at.isoformat(),
            "read_at": self.read_at.isoformat() if self.read_at else None
        }
    
    def __repr__(self):
        return f"Notification(id={self.id}, title={self.title})"

