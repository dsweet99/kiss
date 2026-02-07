"""Examples of overly nested code - too many indentation levels."""


def process_user_request(request, user, permissions, config):
    """This function has way too much nesting."""
    result = {"success": False, "data": None, "errors": []}
    
    if request is not None:
        if request.get("action") is not None:
            action = request["action"]
            if action in ["create", "update", "delete", "read"]:
                if user is not None:
                    if user.get("is_active"):
                        if permissions is not None:
                            if action in permissions:
                                if permissions[action] == True:
                                    if config is not None:
                                        if config.get("feature_enabled"):
                                            if request.get("data") is not None:
                                                data = request["data"]
                                                if isinstance(data, dict):
                                                    if "id" in data:
                                                        if data["id"] is not None:
                                                            # Finally do the actual work!
                                                            result["success"] = True
                                                            result["data"] = {"processed": True, "id": data["id"]}
                                                        else:
                                                            result["errors"].append("ID cannot be null")
                                                    else:
                                                        result["errors"].append("Missing ID in data")
                                                else:
                                                    result["errors"].append("Data must be a dictionary")
                                            else:
                                                result["errors"].append("No data provided")
                                        else:
                                            result["errors"].append("Feature is disabled")
                                    else:
                                        result["errors"].append("No config provided")
                                else:
                                    result["errors"].append("Permission denied for action")
                            else:
                                result["errors"].append("Action not in permissions")
                        else:
                            result["errors"].append("No permissions provided")
                    else:
                        result["errors"].append("User is not active")
                else:
                    result["errors"].append("No user provided")
            else:
                result["errors"].append("Invalid action")
        else:
            result["errors"].append("No action specified")
    else:
        result["errors"].append("No request provided")
    
    return result


def calculate_shipping(order, customer, warehouse, carrier_rates, promotions, settings):
    """Another deeply nested function."""
    shipping_cost = 0.0
    
    if order:
        if order.get("items"):
            if len(order["items"]) > 0:
                if customer:
                    if customer.get("address"):
                        address = customer["address"]
                        if address.get("country"):
                            if address.get("zip_code"):
                                if warehouse:
                                    if warehouse.get("location"):
                                        if carrier_rates:
                                            for carrier, rates in carrier_rates.items():
                                                if rates:
                                                    if address["country"] in rates:
                                                        country_rates = rates[address["country"]]
                                                        if country_rates:
                                                            if "base_rate" in country_rates:
                                                                base = country_rates["base_rate"]
                                                                if "per_kg" in country_rates:
                                                                    per_kg = country_rates["per_kg"]
                                                                    total_weight = sum(
                                                                        item.get("weight", 0) * item.get("quantity", 1)
                                                                        for item in order["items"]
                                                                    )
                                                                    shipping_cost = base + (total_weight * per_kg)
                                                                    
                                                                    if promotions:
                                                                        if "free_shipping" in promotions:
                                                                            promo = promotions["free_shipping"]
                                                                            if promo.get("enabled"):
                                                                                if promo.get("min_order"):
                                                                                    order_total = sum(
                                                                                        item.get("price", 0) * item.get("quantity", 1)
                                                                                        for item in order["items"]
                                                                                    )
                                                                                    if order_total >= promo["min_order"]:
                                                                                        shipping_cost = 0.0
                                                                    break
    return shipping_cost


def validate_form_submission(form_data, schema, rules, context, options):
    """Yet another deeply nested validation function."""
    errors = []
    warnings = []
    
    if form_data:
        if isinstance(form_data, dict):
            if schema:
                if isinstance(schema, dict):
                    for field_name, field_schema in schema.items():
                        if field_schema:
                            if field_schema.get("required"):
                                if field_name not in form_data:
                                    errors.append(f"Missing required field: {field_name}")
                                else:
                                    value = form_data[field_name]
                                    if value is not None:
                                        if field_schema.get("type"):
                                            expected_type = field_schema["type"]
                                            if expected_type == "string":
                                                if not isinstance(value, str):
                                                    errors.append(f"{field_name} must be a string")
                                                else:
                                                    if field_schema.get("min_length"):
                                                        if len(value) < field_schema["min_length"]:
                                                            errors.append(f"{field_name} too short")
                                                    if field_schema.get("max_length"):
                                                        if len(value) > field_schema["max_length"]:
                                                            errors.append(f"{field_name} too long")
                                                    if field_schema.get("pattern"):
                                                        import re
                                                        if not re.match(field_schema["pattern"], value):
                                                            errors.append(f"{field_name} invalid format")
                                            elif expected_type == "number":
                                                if not isinstance(value, (int, float)):
                                                    errors.append(f"{field_name} must be a number")
                                                else:
                                                    if field_schema.get("min"):
                                                        if value < field_schema["min"]:
                                                            errors.append(f"{field_name} below minimum")
                                                    if field_schema.get("max"):
                                                        if value > field_schema["max"]:
                                                            errors.append(f"{field_name} above maximum")
                                    else:
                                        if field_schema.get("nullable") != True:
                                            errors.append(f"{field_name} cannot be null")
    
    return {"valid": len(errors) == 0, "errors": errors, "warnings": warnings}

