#!/usr/bin/env python3
"""
Test with realistic queries for the actual database schema
"""

import psycopg2

CONN_STRING = "postgresql://postgres:BRekrvFMzlBZkuGrJogNBRVFnFQWLqZf@nozomi.proxy.rlwy.net:59255/railway"

# Realistic test queries based on actual schema
REALISTIC_QUERIES = [
    "show me all products",
    "get the first 10 retailers",
    "find all products with prices over 100",
    "count how many merchant leads we have",
    "show me recent crawl jobs from the last day",
    "get all product variants for product_id 123",
]

def get_tables():
    """Get table names"""
    conn = psycopg2.connect(CONN_STRING)
    cur = conn.cursor()
    cur.execute("SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' ORDER BY table_name")
    tables = [row[0] for row in cur.fetchall()]
    cur.close()
    conn.close()
    return tables

def build_prompt(nl_query: str, tables: list) -> str:
    """Build prompt"""
    context = "Database Type: Postgres\nDatabase: railway\n\nAvailable Tables/Collections:\n"
    for table in tables:
        context += f"- {table}\n"
    
    return f"""You are a database query assistant. Convert natural language requests into valid database queries.

{context}

Rules:
- For PostgreSQL: Generate valid SQL SELECT statements
- For MongoDB: Generate valid MongoDB query JSON
- Return ONLY the query, no explanations
- Use proper table/collection names from the schema
- Be concise and accurate

User request: {nl_query}"""

if __name__ == "__main__":
    print("🔗 Getting database schema...")
    tables = get_tables()
    print(f"✅ Found tables: {', '.join(tables)}\n")
    
    print("=" * 70)
    print("Testing Realistic Natural Language Queries")
    print("=" * 70)
    
    for i, query in enumerate(REALISTIC_QUERIES, 1):
        print(f"\n[{i}] Query: \"{query}\"")
        print("-" * 70)
        prompt = build_prompt(query, tables)
        print("Prompt ready ✅")
        print(f"Prompt length: {len(prompt)} characters")
    
    print("\n" + "=" * 70)
    print("✅ All prompts generated successfully!")
    print("=" * 70)
    print("\n📋 Database Schema:")
    for table in tables:
        print(f"   ✓ {table}")
    
    print("\n💡 In the Forge IDE:")
    print("   1. Connect to this PostgreSQL database")
    print("   2. Type any of these natural language queries")
    print("   3. Click '✨ Ask AI' button")
    print("   4. AI will convert to SQL and execute automatically!")
