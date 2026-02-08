#!/usr/bin/env python3
"""
Test the natural language to SQL prompt generation
Shows what prompt will be sent to the AI
"""

import psycopg2

# Database connection
CONN_STRING = "postgresql://postgres:BRekrvFMzlBZkuGrJogNBRVFnFQWLqZf@nozomi.proxy.rlwy.net:59255/railway"

# Test queries
TEST_QUERIES = [
    "show me all tables",
    "get the first 10 users",
    "find users created in the last week",
    "count total number of records in users table",
]

def get_tables():
    """Get table names from PostgreSQL"""
    try:
        print("🔗 Connecting to database...")
        conn = psycopg2.connect(CONN_STRING)
        cur = conn.cursor()
        
        cur.execute("""
            SELECT table_name, table_type
            FROM information_schema.tables 
            WHERE table_schema = 'public' 
            ORDER BY table_name
        """)
        
        tables = []
        for row in cur.fetchall():
            tables.append({"name": row[0], "type": row[1]})
        
        cur.close()
        conn.close()
        
        print(f"✅ Connected! Found {len(tables)} tables\n")
        return tables
        
    except Exception as e:
        print(f"❌ Connection failed: {e}\n")
        return []

def build_prompt(nl_query: str, tables: list) -> str:
    """Build the exact prompt that Rust code generates"""
    context = "Database Type: Postgres\n"
    context += "Database: railway\n"
    
    if tables:
        context += "\nAvailable Tables/Collections:\n"
        for table in tables:
            context += f"- {table['name']}\n"
    
    prompt = f"""You are a database query assistant. Convert natural language requests into valid database queries.

{context}

Rules:
- For PostgreSQL: Generate valid SQL SELECT statements
- For MongoDB: Generate valid MongoDB query JSON
- Return ONLY the query, no explanations
- Use proper table/collection names from the schema
- Be concise and accurate

User request: {nl_query}"""
    
    return prompt

if __name__ == "__main__":
    print("=" * 70)
    print("Natural Language to SQL - Prompt Generation Test")
    print("=" * 70)
    print()
    
    # Get actual tables from database
    tables = get_tables()
    
    if tables:
        print("📋 Tables found:")
        for table in tables:
            print(f"   - {table['name']} ({table['type']})")
        print()
    
    # Generate prompts for each test query
    print("=" * 70)
    print("Generated Prompts (sent to AI)")
    print("=" * 70)
    
    for i, nl_query in enumerate(TEST_QUERIES, 1):
        print(f"\n{'=' * 70}")
        print(f"[Test {i}] Natural Language: \"{nl_query}\"")
        print('=' * 70)
        
        prompt = build_prompt(nl_query, tables)
        
        print("\n📤 PROMPT THAT WILL BE SENT TO AI:")
        print("-" * 70)
        print(prompt)
        print("-" * 70)
        
        print("\n💡 Expected SQL output (example):")
        if "show me all tables" in nl_query.lower():
            print("   SELECT table_name FROM information_schema.tables WHERE table_schema = 'public';")
        elif "first 10 users" in nl_query.lower():
            print("   SELECT * FROM users LIMIT 10;")
        elif "created in the last week" in nl_query.lower():
            print("   SELECT * FROM users WHERE created_at >= NOW() - INTERVAL '7 days';")
        elif "count" in nl_query.lower():
            print("   SELECT COUNT(*) FROM users;")
        
        print()
    
    print("\n" + "=" * 70)
    print("✅ Prompt generation test complete!")
    print("=" * 70)
    print("\n💡 These prompts are what get sent to your configured AI provider")
    print("   (Gemini/Anthropic/OpenAI) to generate the actual SQL queries.")
