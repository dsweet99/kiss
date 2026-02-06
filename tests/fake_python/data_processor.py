"""Data processor with overly long functions."""

import json
import csv
import os
from datetime import datetime


def process_data_file(filepath, output_dir, config):
    """
    This function is WAY too long. It does:
    - File validation
    - Format detection
    - Parsing
    - Transformation
    - Validation
    - Output writing
    - Logging
    - Cleanup
    
    Should be broken into many smaller functions.
    """
    # Step 1: Validate input file exists
    if not filepath:
        print("Error: No filepath provided")
        return None
    
    if not os.path.exists(filepath):
        print(f"Error: File not found: {filepath}")
        return None
    
    if not os.path.isfile(filepath):
        print(f"Error: Not a file: {filepath}")
        return None
    
    file_size = os.path.getsize(filepath)
    if file_size == 0:
        print(f"Error: File is empty: {filepath}")
        return None
    
    if file_size > config.get("max_file_size", 100_000_000):
        print(f"Error: File too large: {file_size} bytes")
        return None
    
    # Step 2: Detect file format
    extension = os.path.splitext(filepath)[1].lower()
    
    if extension == ".json":
        file_format = "json"
    elif extension == ".csv":
        file_format = "csv"
    elif extension == ".tsv":
        file_format = "tsv"
    elif extension == ".txt":
        file_format = "text"
    else:
        print(f"Error: Unsupported file format: {extension}")
        return None
    
    print(f"Detected format: {file_format}")
    
    # Step 3: Parse the file
    data = []
    errors = []
    
    if file_format == "json":
        try:
            with open(filepath, 'r', encoding='utf-8') as f:
                content = f.read()
                parsed = json.loads(content)
                if isinstance(parsed, list):
                    data = parsed
                elif isinstance(parsed, dict):
                    data = [parsed]
                else:
                    errors.append("JSON must be object or array")
        except json.JSONDecodeError as e:
            errors.append(f"JSON parse error: {e}")
        except UnicodeDecodeError as e:
            errors.append(f"Encoding error: {e}")
    
    elif file_format == "csv":
        try:
            with open(filepath, 'r', encoding='utf-8') as f:
                reader = csv.DictReader(f)
                for row_num, row in enumerate(reader, start=2):
                    if any(v.strip() for v in row.values()):
                        data.append(dict(row))
                    else:
                        errors.append(f"Empty row at line {row_num}")
        except csv.Error as e:
            errors.append(f"CSV parse error: {e}")
    
    elif file_format == "tsv":
        try:
            with open(filepath, 'r', encoding='utf-8') as f:
                reader = csv.DictReader(f, delimiter='\t')
                for row_num, row in enumerate(reader, start=2):
                    if any(v.strip() for v in row.values()):
                        data.append(dict(row))
                    else:
                        errors.append(f"Empty row at line {row_num}")
        except csv.Error as e:
            errors.append(f"TSV parse error: {e}")
    
    elif file_format == "text":
        try:
            with open(filepath, 'r', encoding='utf-8') as f:
                for line_num, line in enumerate(f, start=1):
                    line = line.strip()
                    if line:
                        data.append({"line_number": line_num, "content": line})
        except UnicodeDecodeError as e:
            errors.append(f"Encoding error: {e}")
    
    if errors:
        print(f"Parse errors: {errors}")
    
    print(f"Parsed {len(data)} records")
    
    # Step 4: Transform data
    transformed = []
    transform_errors = []
    
    for i, record in enumerate(data):
        try:
            new_record = {}
            
            for key, value in record.items():
                clean_key = key.strip().lower().replace(" ", "_")
                
                if isinstance(value, str):
                    value = value.strip()
                    if value.lower() in ('null', 'none', 'na', 'n/a', ''):
                        value = None
                    elif value.lower() in ('true', 'yes', '1'):
                        value = True
                    elif value.lower() in ('false', 'no', '0'):
                        value = False
                    else:
                        try:
                            if '.' in value:
                                value = float(value)
                            else:
                                value = int(value)
                        except ValueError:
                            pass
                
                new_record[clean_key] = value
            
            new_record["_source_file"] = os.path.basename(filepath)
            new_record["_processed_at"] = datetime.now().isoformat()
            new_record["_record_index"] = i
            
            transformed.append(new_record)
            
        except Exception as e:
            transform_errors.append(f"Record {i}: {e}")
    
    if transform_errors:
        print(f"Transform errors: {transform_errors}")
    
    print(f"Transformed {len(transformed)} records")
    
    # Step 5: Validate transformed data
    validated = []
    validation_errors = []
    
    required_fields = config.get("required_fields", [])
    
    for i, record in enumerate(transformed):
        is_valid = True
        
        for field in required_fields:
            if field not in record or record[field] is None:
                validation_errors.append(f"Record {i}: missing required field '{field}'")
                is_valid = False
        
        if "email" in record and record["email"]:
            email = record["email"]
            if "@" not in email or "." not in email:
                validation_errors.append(f"Record {i}: invalid email '{email}'")
                is_valid = False
        
        if "age" in record and record["age"] is not None:
            age = record["age"]
            if not isinstance(age, (int, float)) or age < 0 or age > 150:
                validation_errors.append(f"Record {i}: invalid age '{age}'")
                is_valid = False
        
        if is_valid:
            validated.append(record)
    
    if validation_errors:
        print(f"Validation errors ({len(validation_errors)}):")
        for err in validation_errors[:10]:
            print(f"  {err}")
        if len(validation_errors) > 10:
            print(f"  ... and {len(validation_errors) - 10} more")
    
    print(f"Validated {len(validated)} records")
    
    # Step 6: Write output
    if not os.path.exists(output_dir):
        os.makedirs(output_dir)
    
    base_name = os.path.splitext(os.path.basename(filepath))[0]
    output_path = os.path.join(output_dir, f"{base_name}_processed.json")
    
    try:
        with open(output_path, 'w', encoding='utf-8') as f:
            json.dump({
                "metadata": {
                    "source_file": filepath,
                    "processed_at": datetime.now().isoformat(),
                    "total_records": len(data),
                    "valid_records": len(validated),
                    "parse_errors": len(errors),
                    "transform_errors": len(transform_errors),
                    "validation_errors": len(validation_errors)
                },
                "records": validated
            }, f, indent=2, default=str)
        print(f"Output written to: {output_path}")
    except IOError as e:
        print(f"Error writing output: {e}")
        return None
    
    # Step 7: Write error log
    error_log_path = os.path.join(output_dir, f"{base_name}_errors.log")
    all_errors = errors + transform_errors + validation_errors
    
    if all_errors:
        try:
            with open(error_log_path, 'w', encoding='utf-8') as f:
                f.write(f"Error log for {filepath}\n")
                f.write(f"Generated at {datetime.now().isoformat()}\n")
                f.write("=" * 50 + "\n\n")
                for error in all_errors:
                    f.write(f"{error}\n")
            print(f"Error log written to: {error_log_path}")
        except IOError as e:
            print(f"Error writing error log: {e}")
    
    # Step 8: Cleanup and return
    result = {
        "input_file": filepath,
        "output_file": output_path,
        "total_records": len(data),
        "valid_records": len(validated),
        "error_count": len(all_errors),
        "success": len(validated) > 0
    }
    
    print(f"Processing complete: {result}")
    return result

