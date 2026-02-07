"""Report generator - copy-pasted formatting code across methods."""

from datetime import datetime
from typing import Any


class ReportGenerator:
    """Generates various reports - lots of duplicated formatting logic."""

    def __init__(self, config):
        self.config = config
        self.company_name = config.get("company_name", "ACME Corp")
        self.date_format = config.get("date_format", "%Y-%m-%d")

    def generate_sales_report(self, sales_data: list[dict]) -> str:
        """Generate sales report with duplicated header/footer formatting."""
        lines = []
        
        # Header - DUPLICATED BLOCK A
        lines.append("=" * 60)
        lines.append(f"  {self.company_name}")
        lines.append(f"  Report Generated: {datetime.now().strftime(self.date_format)}")
        lines.append("=" * 60)
        lines.append("")
        lines.append("  SALES REPORT")
        lines.append("-" * 60)
        lines.append("")
        
        # Content
        total_sales = 0.0
        for sale in sales_data:
            date = sale.get("date", "N/A")
            product = sale.get("product", "Unknown")
            quantity = sale.get("quantity", 0)
            unit_price = sale.get("unit_price", 0.0)
            total = quantity * unit_price
            total_sales += total
            
            lines.append(f"  {date}  {product:<20}  {quantity:>5} x ${unit_price:>8.2f} = ${total:>10.2f}")
        
        lines.append("")
        lines.append("-" * 60)
        lines.append(f"  TOTAL SALES: ${total_sales:>10.2f}")
        
        # Footer - DUPLICATED BLOCK B
        lines.append("")
        lines.append("=" * 60)
        lines.append(f"  End of Report")
        lines.append(f"  Page 1 of 1")
        lines.append(f"  Confidential - {self.company_name}")
        lines.append("=" * 60)
        
        return "\n".join(lines)

    def generate_inventory_report(self, inventory_data: list[dict]) -> str:
        """Generate inventory report - same header/footer as sales report."""
        lines = []
        
        # Header - DUPLICATED BLOCK A (copy-pasted!)
        lines.append("=" * 60)
        lines.append(f"  {self.company_name}")
        lines.append(f"  Report Generated: {datetime.now().strftime(self.date_format)}")
        lines.append("=" * 60)
        lines.append("")
        lines.append("  INVENTORY REPORT")
        lines.append("-" * 60)
        lines.append("")
        
        # Content
        total_value = 0.0
        low_stock_items = []
        
        for item in inventory_data:
            sku = item.get("sku", "N/A")
            name = item.get("name", "Unknown")
            quantity = item.get("quantity", 0)
            unit_cost = item.get("unit_cost", 0.0)
            reorder_level = item.get("reorder_level", 10)
            value = quantity * unit_cost
            total_value += value
            
            status = "OK" if quantity > reorder_level else "LOW"
            if status == "LOW":
                low_stock_items.append(name)
            
            lines.append(f"  {sku:<10}  {name:<20}  {quantity:>5} @ ${unit_cost:>8.2f} = ${value:>10.2f}  [{status}]")
        
        lines.append("")
        lines.append("-" * 60)
        lines.append(f"  TOTAL INVENTORY VALUE: ${total_value:>10.2f}")
        if low_stock_items:
            lines.append(f"  LOW STOCK ITEMS: {', '.join(low_stock_items)}")
        
        # Footer - DUPLICATED BLOCK B (copy-pasted!)
        lines.append("")
        lines.append("=" * 60)
        lines.append(f"  End of Report")
        lines.append(f"  Page 1 of 1")
        lines.append(f"  Confidential - {self.company_name}")
        lines.append("=" * 60)
        
        return "\n".join(lines)

    def generate_employee_report(self, employee_data: list[dict]) -> str:
        """Generate employee report - again, same header/footer."""
        lines = []
        
        # Header - DUPLICATED BLOCK A (third copy!)
        lines.append("=" * 60)
        lines.append(f"  {self.company_name}")
        lines.append(f"  Report Generated: {datetime.now().strftime(self.date_format)}")
        lines.append("=" * 60)
        lines.append("")
        lines.append("  EMPLOYEE REPORT")
        lines.append("-" * 60)
        lines.append("")
        
        # Content
        total_salary = 0.0
        departments = {}
        
        for emp in employee_data:
            emp_id = emp.get("id", "N/A")
            name = emp.get("name", "Unknown")
            department = emp.get("department", "Unassigned")
            salary = emp.get("salary", 0.0)
            hire_date = emp.get("hire_date", "N/A")
            total_salary += salary
            
            departments[department] = departments.get(department, 0) + 1
            
            lines.append(f"  {emp_id:<6}  {name:<25}  {department:<15}  ${salary:>10.2f}  {hire_date}")
        
        lines.append("")
        lines.append("-" * 60)
        lines.append(f"  TOTAL PAYROLL: ${total_salary:>10.2f}")
        lines.append(f"  HEADCOUNT BY DEPARTMENT:")
        for dept, count in sorted(departments.items()):
            lines.append(f"    {dept}: {count}")
        
        # Footer - DUPLICATED BLOCK B (third copy!)
        lines.append("")
        lines.append("=" * 60)
        lines.append(f"  End of Report")
        lines.append(f"  Page 1 of 1")
        lines.append(f"  Confidential - {self.company_name}")
        lines.append("=" * 60)
        
        return "\n".join(lines)

    def generate_expense_report(self, expense_data: list[dict]) -> str:
        """Generate expense report - you guessed it, same header/footer."""
        lines = []
        
        # Header - DUPLICATED BLOCK A (fourth copy!)
        lines.append("=" * 60)
        lines.append(f"  {self.company_name}")
        lines.append(f"  Report Generated: {datetime.now().strftime(self.date_format)}")
        lines.append("=" * 60)
        lines.append("")
        lines.append("  EXPENSE REPORT")
        lines.append("-" * 60)
        lines.append("")
        
        # Content
        total_expenses = 0.0
        categories = {}
        
        for expense in expense_data:
            date = expense.get("date", "N/A")
            description = expense.get("description", "Unknown")
            category = expense.get("category", "Other")
            amount = expense.get("amount", 0.0)
            approved = expense.get("approved", False)
            
            total_expenses += amount
            categories[category] = categories.get(category, 0.0) + amount
            
            status = "✓" if approved else "○"
            lines.append(f"  {date}  {description:<30}  {category:<15}  ${amount:>8.2f}  {status}")
        
        lines.append("")
        lines.append("-" * 60)
        lines.append(f"  TOTAL EXPENSES: ${total_expenses:>10.2f}")
        lines.append(f"  BY CATEGORY:")
        for cat, total in sorted(categories.items(), key=lambda x: -x[1]):
            lines.append(f"    {cat}: ${total:.2f}")
        
        # Footer - DUPLICATED BLOCK B (fourth copy!)
        lines.append("")
        lines.append("=" * 60)
        lines.append(f"  End of Report")
        lines.append(f"  Page 1 of 1")
        lines.append(f"  Confidential - {self.company_name}")
        lines.append("=" * 60)
        
        return "\n".join(lines)

