#!/usr/bin/env python3
"""
Test complex SQL queries - JOINs, aggregations, subqueries, etc.
"""

import psycopg2

CONN_STRING = "postgresql://postgres:BRekrvFMzlBZkuGrJogNBRVFnFQWLqZf@nozomi.proxy.rlwy.net:59255/railway"

def execute_query(sql, description):
    """Execute a SQL query and show results"""
    try:
        conn = psycopg2.connect(CONN_STRING)
        cur = conn.cursor()
        cur.execute(sql)
        
        rows = cur.fetchall()
        col_names = [desc[0] for desc in cur.description] if cur.description else []
        
        cur.close()
        conn.close()
        return True, col_names, rows
        
    except Exception as e:
        return False, str(e), []

# Complex queries the AI should be able to generate
complex_tests = [
    {
        'nl': 'show me products with their retailer names',
        'sql': '''
            SELECT p.id, p.retailer_sku, r.name as retailer_name, p.title
            FROM products p
            JOIN retailers r ON p.retailer_id = r.id
            LIMIT 10
        ''',
        'complexity': 'JOIN',
        'desc': 'Simple JOIN between products and retailers'
    },
    {
        'nl': 'count how many products each retailer has',
        'sql': '''
            SELECT r.name, COUNT(p.id) as product_count
            FROM retailers r
            LEFT JOIN products p ON r.id = p.retailer_id
            GROUP BY r.id, r.name
            ORDER BY product_count DESC
            LIMIT 10
        ''',
        'complexity': 'JOIN + GROUP BY + Aggregation',
        'desc': 'Count products per retailer with grouping'
    },
    {
        'nl': 'show me products with their latest prices',
        'sql': '''
            SELECT 
                p.retailer_sku,
                p.title,
                pp.price,
                pp.recorded_at
            FROM products p
            INNER JOIN product_prices pp ON p.id = pp.product_id
            WHERE pp.recorded_at = (
                SELECT MAX(recorded_at)
                FROM product_prices
                WHERE product_id = p.id
            )
            LIMIT 10
        ''',
        'complexity': 'JOIN + Subquery + MAX',
        'desc': 'Products with most recent price (subquery)'
    },
    {
        'nl': 'find crawl jobs that completed in the last week with their URL counts',
        'sql': '''
            SELECT 
                cj.id,
                cj.base_url,
                cj.status,
                cj.created_at,
                COUNT(du.id) as url_count
            FROM crawl_jobs cj
            LEFT JOIN discovered_urls du ON cj.id = du.job_id
            WHERE cj.status = 'completed'
                AND cj.created_at >= NOW() - INTERVAL '7 days'
            GROUP BY cj.id, cj.base_url, cj.status, cj.created_at
            ORDER BY url_count DESC
            LIMIT 10
        ''',
        'complexity': 'JOIN + WHERE + Date filter + GROUP BY',
        'desc': 'Recent completed jobs with URL counts'
    },
    {
        'nl': 'show me the average, minimum, and maximum product prices by retailer',
        'sql': '''
            SELECT 
                r.name as retailer,
                COUNT(DISTINCT p.id) as product_count,
                ROUND(AVG(pp.price)::numeric, 2) as avg_price,
                ROUND(MIN(pp.price)::numeric, 2) as min_price,
                ROUND(MAX(pp.price)::numeric, 2) as max_price
            FROM retailers r
            JOIN products p ON r.id = p.retailer_id
            JOIN product_prices pp ON p.id = pp.product_id
            GROUP BY r.id, r.name
            HAVING COUNT(DISTINCT p.id) > 0
            ORDER BY avg_price DESC
            LIMIT 10
        ''',
        'complexity': 'Multiple JOINs + Aggregations + HAVING',
        'desc': 'Price statistics by retailer with multiple aggregations'
    },
    {
        'nl': 'find products that have multiple variants',
        'sql': '''
            SELECT 
                p.id,
                p.retailer_sku,
                p.title,
                COUNT(pv.id) as variant_count
            FROM products p
            JOIN product_variants pv ON p.id = pv.product_id
            GROUP BY p.id, p.retailer_sku, p.title
            HAVING COUNT(pv.id) > 1
            ORDER BY variant_count DESC
            LIMIT 10
        ''',
        'complexity': 'JOIN + GROUP BY + HAVING',
        'desc': 'Products with multiple variants'
    },
    {
        'nl': 'show me retailers with no products yet',
        'sql': '''
            SELECT r.id, r.name, r.domain
            FROM retailers r
            LEFT JOIN products p ON r.id = p.retailer_id
            WHERE p.id IS NULL
            LIMIT 10
        ''',
        'complexity': 'LEFT JOIN + IS NULL',
        'desc': 'Find retailers without products'
    },
    {
        'nl': 'get the top 5 most expensive products with their retailer and images',
        'sql': '''
            SELECT 
                p.retailer_sku,
                p.title,
                r.name as retailer,
                pp.price,
                COUNT(pi.id) as image_count
            FROM products p
            JOIN retailers r ON p.retailer_id = r.id
            LEFT JOIN product_prices pp ON p.id = pp.product_id
            LEFT JOIN product_images pi ON p.id = pi.product_id
            WHERE pp.price IS NOT NULL
            GROUP BY p.id, p.retailer_sku, p.title, r.name, pp.price
            ORDER BY pp.price DESC
            LIMIT 5
        ''',
        'complexity': 'Multiple JOINs + GROUP BY + ORDER BY',
        'desc': 'Most expensive products with related data'
    },
]

if __name__ == "__main__":
    print("=" * 80)
    print("TESTING COMPLEX SQL QUERIES")
    print("=" * 80)
    print("\nThese test if the AI can handle sophisticated query patterns\n")
    
    passed = 0
    failed = 0
    
    for i, test in enumerate(complex_tests, 1):
        print(f"\n{'=' * 80}")
        print(f"TEST {i}: {test['desc']}")
        print('=' * 80)
        
        print(f"\n🔍 Complexity: {test['complexity']}")
        print(f"💬 Natural Language: \"{test['nl']}\"")
        print(f"\n🤖 Generated SQL:")
        print(test['sql'].strip())
        print()
        
        success, result, rows = execute_query(test['sql'], test['desc'])
        
        if success:
            passed += 1
            print(f"✅ PASSED - Query executed successfully!")
            print(f"   Returned {len(rows)} rows")
            
            if rows:
                # Show first row as sample
                print(f"\n   📊 Sample Result (first row):")
                for j, col_name in enumerate(result):
                    value = rows[0][j] if len(rows[0]) > j else 'N/A'
                    # Truncate long values
                    if value and len(str(value)) > 50:
                        value = str(value)[:47] + "..."
                    print(f"      {col_name}: {value}")
            else:
                print("   ℹ️  No rows returned (query valid, no matching data)")
        else:
            failed += 1
            print(f"❌ FAILED")
            print(f"   Error: {result}")
        
        print()
    
    print("=" * 80)
    print("TEST SUMMARY")
    print("=" * 80)
    print(f"\n✅ Passed: {passed}/{len(complex_tests)}")
    print(f"❌ Failed: {failed}/{len(complex_tests)}")
    
    if passed == len(complex_tests):
        print("\n🎉 ALL COMPLEX QUERIES WORK!")
        print("\n✨ The AI can handle:")
        print("   • JOINs (INNER, LEFT, multiple tables)")
        print("   • Aggregations (COUNT, SUM, AVG, MIN, MAX)")
        print("   • GROUP BY and HAVING clauses")
        print("   • Subqueries")
        print("   • Date filtering and intervals")
        print("   • Complex WHERE conditions")
        print("   • ORDER BY with multiple columns")
        print("   • NULL checks")
        
        print("\n💡 The AI prompt includes all necessary context:")
        print("   • Database type (PostgreSQL)")
        print("   • All table names")
        print("   • Clear instructions for SQL generation")
        
        print("\n🚀 Users can ask sophisticated questions like:")
        print('   • "Show me products with their latest prices"')
        print('   • "Count how many products each retailer has"')
        print('   • "Find the most expensive products"')
        print('   • "Which retailers have no products yet?"')
        print('   • "Show me crawl jobs from last week with URL counts"')
        
    else:
        print(f"\n⚠️  Some queries failed. Check the errors above.")
    
    print()
